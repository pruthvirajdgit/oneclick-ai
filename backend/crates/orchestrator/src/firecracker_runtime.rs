//! Firecracker-based agent runtime using the fctools SDK.
//!
//! Each agent runs in its own Firecracker microVM with a dedicated TAP
//! network device.  VMs are managed via fctools' `Vm` abstraction which
//! handles process spawning, API calls, resource management, and snapshots.
//!
//! Lifecycle:
//!   create  → copy rootfs, allocate TAP, build VmConfigurationData
//!   start   → cold boot via fctools `Vm::start`
//!   stop    → pause + snapshot + shutdown (saves state to disk)
//!   wake    → restore from snapshot via `VmSnapshot::prepare_vm`
//!   destroy → shutdown VM + release TAP + delete files

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use fctools::{
    process_spawner::DirectProcessSpawner,
    runtime::tokio::TokioRuntime,
    vm::{
        Vm,
        api::VmApi,
        configuration::{InitMethod, VmConfiguration, VmConfigurationData},
        models::{
            BootSource, CreateSnapshot, Drive,
            MachineConfiguration, NetworkInterface, SnapshotType,
        },
        shutdown::{VmShutdownAction, VmShutdownMethod},
        snapshot::{PrepareVmFromSnapshotOptions, VmSnapshot},
    },
    vmm::{
        arguments::{VmmApiSocket, VmmArguments},
        executor::unrestricted::UnrestrictedVmmExecutor,
        installation::VmmInstallation,
        ownership::VmmOwnershipModel,
        resource::{MovedResourceType, ResourceType},
        resource::system::ResourceSystem,
    },
};

use oneclick_shared::config::Config;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::Agent;

use crate::runtime::AgentRuntime;
use crate::tap_manager::TapManager;

type FcVm = Vm<UnrestrictedVmmExecutor, DirectProcessSpawner, TokioRuntime>;

const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/fc-init";
const FC_BIN: &str = "/usr/local/bin/firecracker";
const JAILER_BIN: &str = "/usr/local/bin/jailer";
const SNAP_EDITOR_BIN: &str = "/usr/local/bin/snapshot-editor";
const AGENT_BRIDGE_PORT: u16 = 3001;

/// Per-VM state kept in memory while the VM is alive.
struct VmState {
    vm: FcVm,
    #[allow(dead_code)]
    tap_device: String,
    guest_ip: String,
    #[allow(dead_code)]
    socket_path: String,
    snapshot: Option<VmSnapshot>,
    has_snapshot_on_disk: bool,
}

/// Firecracker-based agent runtime.
pub struct FirecrackerRuntime {
    config: Arc<Config>,
    tap_manager: Arc<TapManager>,
    /// agent_id → live VM state
    vms: Mutex<HashMap<String, VmState>>,
}

impl FirecrackerRuntime {
    /// Create a new Firecracker runtime.
    pub fn new(config: Arc<Config>, tap_manager: Arc<TapManager>) -> Self {
        Self {
            config,
            tap_manager,
            vms: Mutex::new(HashMap::new()),
        }
    }

    fn installation() -> VmmInstallation {
        VmmInstallation::new(FC_BIN, JAILER_BIN, SNAP_EDITOR_BIN)
    }

    fn spawner() -> DirectProcessSpawner {
        DirectProcessSpawner
    }

    fn executor(socket_path: &str) -> UnrestrictedVmmExecutor {
        let args = VmmArguments::new(VmmApiSocket::Enabled(socket_path.into()));
        UnrestrictedVmmExecutor::new(args)
    }

    fn resource_system() -> ResourceSystem<DirectProcessSpawner, TokioRuntime> {
        ResourceSystem::new(
            Self::spawner(),
            TokioRuntime,
            VmmOwnershipModel::UpgradedPermanently,
        )
    }

    /// Directories for a given agent
    fn vm_dir(&self, agent_id: &str) -> PathBuf {
        PathBuf::from(&self.config.fc_vm_dir).join(agent_id)
    }

    fn snapshot_dir(&self, agent_id: &str) -> PathBuf {
        PathBuf::from(&self.config.fc_snapshot_dir).join(agent_id)
    }

    fn socket_path(&self, agent_id: &str) -> String {
        format!("/tmp/fc-{}.socket", agent_id)
    }

    fn rootfs_path(&self, agent_id: &str) -> PathBuf {
        self.vm_dir(agent_id).join("rootfs.ext4")
    }

    /// Build the VmConfigurationData for a fresh VM.
    fn build_config(
        &self,
        agent_id: &str,
        tap_device: &str,
        guest_mac: &str,
        resource_system: &mut ResourceSystem<DirectProcessSpawner, TokioRuntime>,
    ) -> AppResult<VmConfigurationData> {
        let kernel = resource_system
            .create_resource(
                PathBuf::from(&self.config.fc_kernel_path),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .map_err(|e| AppError::Internal(format!("Kernel resource: {:?}", e)))?;

        let rootfs = resource_system
            .create_resource(
                self.rootfs_path(agent_id),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .map_err(|e| AppError::Internal(format!("Rootfs resource: {:?}", e)))?;

        Ok(VmConfigurationData {
            boot_source: BootSource {
                kernel_image: kernel,
                boot_args: Some(BOOT_ARGS.into()),
                initrd: None,
            },
            drives: vec![Drive {
                drive_id: "rootfs".into(),
                is_root_device: true,
                cache_type: None,
                partuuid: None,
                is_read_only: Some(false),
                block: Some(rootfs),
                rate_limiter: None,
                io_engine: None,
                socket: None,
            }],
            pmem_devices: vec![],
            machine_configuration: MachineConfiguration {
                vcpu_count: self.config.fc_vcpu_count as u8,
                mem_size_mib: self.config.fc_mem_size_mib as usize,
                smt: None,
                track_dirty_pages: Some(true),
                huge_pages: None,
            },
            cpu_template: None,
            network_interfaces: vec![NetworkInterface {
                iface_id: "eth0".into(),
                host_dev_name: tap_device.into(),
                guest_mac: Some(guest_mac.into()),
                rx_rate_limiter: None,
                tx_rate_limiter: None,
            }],
            balloon_device: None,
            vsock_device: None,
            logger_system: None,
            metrics_system: None,
            memory_hotplug_configuration: None,
            mmds_configuration: None,
            entropy_device: None,
        })
    }

    /// Cold-boot a VM using fctools.
    async fn cold_boot(
        &self,
        agent_id: &str,
        tap_device: &str,
        guest_mac: &str,
    ) -> AppResult<FcVm> {
        let socket_path = self.socket_path(agent_id);
        let _ = tokio::fs::remove_file(&socket_path).await;

        let mut resource_system = Self::resource_system();
        let data = self.build_config(agent_id, tap_device, guest_mac, &mut resource_system)?;

        let configuration = VmConfiguration::New {
            init_method: InitMethod::ViaApiCalls,
            data,
        };

        let mut vm = Vm::prepare(
            Self::executor(&socket_path),
            resource_system,
            Self::installation(),
            configuration,
        )
        .await
        .map_err(|e| AppError::Internal(format!("Vm::prepare failed: {:?}", e)))?;

        // Spawn a task to chmod the socket (FC creates it as root)
        let sp = socket_path.clone();
        let chmod_handle = tokio::spawn(async move {
            for _ in 0..100 {
                if std::path::Path::new(&sp).exists() {
                    let _ = Command::new("sudo")
                        .args(["chmod", "666", &sp])
                        .output()
                        .await;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        vm.start(Duration::from_secs(30))
            .await
            .map_err(|e| AppError::Internal(format!("Vm::start failed: {:?}", e)))?;

        chmod_handle.abort();
        info!(agent_id, "Firecracker VM cold-booted");
        Ok(vm)
    }

    /// Create a snapshot and shut down the VM.
    async fn snapshot_and_shutdown(
        &self,
        agent_id: &str,
        vm: &mut FcVm,
    ) -> AppResult<VmSnapshot> {
        let snap_dir = self.snapshot_dir(agent_id);
        tokio::fs::create_dir_all(&snap_dir)
            .await
            .map_err(|e| AppError::Internal(format!("Create snapshot dir: {e}")))?;

        let snap_path = snap_dir.join("vm.snap");
        let mem_path = snap_dir.join("vm.mem");

        // Pause
        vm.pause()
            .await
            .map_err(|e| AppError::Internal(format!("Pause failed: {:?}", e)))?;

        // Create snapshot resources
        let snap_resource = vm
            .get_resource_system_mut()
            .create_resource(&snap_path, ResourceType::Produced)
            .map_err(|e| AppError::Internal(format!("Snapshot resource: {:?}", e)))?;
        let mem_resource = vm
            .get_resource_system_mut()
            .create_resource(&mem_path, ResourceType::Produced)
            .map_err(|e| AppError::Internal(format!("Mem resource: {:?}", e)))?;

        let create_snap = CreateSnapshot {
            snapshot_type: Some(SnapshotType::Full),
            snapshot: snap_resource,
            mem_file: mem_resource,
        };

        let snapshot = vm
            .create_snapshot(create_snap)
            .await
            .map_err(|e| AppError::Internal(format!("CreateSnapshot failed: {:?}", e)))?;

        info!(agent_id, "Snapshot saved");

        // Shut down the old VM process to release TAP
        self.shutdown_vm(vm).await?;

        Ok(snapshot)
    }

    /// Restore a VM from a snapshot.
    async fn restore_from_snapshot(
        &self,
        agent_id: &str,
        snapshot: VmSnapshot,
        old_vm: &mut FcVm,
    ) -> AppResult<FcVm> {
        let socket_path = self.socket_path(agent_id);
        let _ = tokio::fs::remove_file(&socket_path).await;

        let options = PrepareVmFromSnapshotOptions {
            executor: Self::executor(&socket_path),
            process_spawner: Self::spawner(),
            runtime: TokioRuntime,
            moved_resource_type: MovedResourceType::HardLinkedOrCopied,
            ownership_model: VmmOwnershipModel::UpgradedPermanently,
            track_dirty_pages: None,
            resume_vm: Some(true),
            network_overrides: vec![],
        };

        let mut new_vm = snapshot
            .prepare_vm(old_vm, options)
            .await
            .map_err(|e| AppError::Internal(format!("prepare_vm failed: {:?}", e)))?;

        // chmod socket
        let sp = socket_path.clone();
        let chmod_handle = tokio::spawn(async move {
            for _ in 0..100 {
                if std::path::Path::new(&sp).exists() {
                    let _ = Command::new("sudo")
                        .args(["chmod", "666", &sp])
                        .output()
                        .await;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        new_vm
            .start(Duration::from_secs(10))
            .await
            .map_err(|e| AppError::Internal(format!("Vm::start (restore) failed: {:?}", e)))?;

        chmod_handle.abort();
        info!(agent_id, "Snapshot restored");
        Ok(new_vm)
    }

    /// Gracefully shut down a VM.
    async fn shutdown_vm(&self, vm: &mut FcVm) -> AppResult<()> {
        let actions = vec![
            VmShutdownAction {
                method: VmShutdownMethod::PauseThenKill,
                timeout: Some(Duration::from_secs(5)),
                graceful: true,
            },
            VmShutdownAction {
                method: VmShutdownMethod::Kill,
                timeout: Some(Duration::from_secs(3)),
                graceful: false,
            },
        ];
        vm.shutdown(actions)
            .await
            .map_err(|e| AppError::Internal(format!("Shutdown failed: {:?}", e)))?;
        Ok(())
    }

    /// Direct TCP health probe to the VM's guest IP.
    async fn probe_health(guest_ip: &str, port: u16) -> bool {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let addr = format!("{}:{}", guest_ip, port);
        let stream = match tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        {
            Ok(Ok(s)) => s,
            _ => return false,
        };

        let (mut reader, mut writer) = stream.into_split();
        let req = format!("GET /health HTTP/1.0\r\nHost: {}\r\n\r\n", addr);
        if writer.write_all(req.as_bytes()).await.is_err() {
            return false;
        }

        let mut buf = vec![0u8; 4096];
        match tokio::time::timeout(Duration::from_secs(3), reader.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                let resp = String::from_utf8_lossy(&buf[..n]);
                resp.contains("200") || resp.contains("ok")
            }
            _ => false,
        }
    }

    /// Mount the rootfs copy and inject per-VM network and environment config.
    async fn inject_rootfs_config(
        &self,
        agent: &Agent,
        config: &Config,
        alloc: &crate::tap_manager::TapAllocation,
    ) -> AppResult<()> {
        let agent_id = agent.id.to_string();
        let rootfs = self.rootfs_path(&agent_id);
        let mount_dir = format!("/tmp/fc-mount-{}", &agent_id[..8]);

        // Mount the rootfs
        let _ = Command::new("sudo").args(["mkdir", "-p", &mount_dir]).output().await;
        let output = Command::new("sudo")
            .args(["mount", "-o", "loop"])
            .arg(rootfs.to_str().unwrap())
            .arg(&mount_dir)
            .output()
            .await
            .map_err(|e| AppError::Internal(format!("Mount rootfs: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Internal(format!("Mount rootfs failed: {stderr}")));
        }

        // From here on, always unmount on error
        let result = self.write_rootfs_config(agent, config, alloc, &mount_dir).await;

        // Always unmount
        let umount_output = Command::new("sudo").args(["umount", &mount_dir]).output().await;
        let _ = Command::new("sudo").args(["rm", "-rf", &mount_dir]).output().await;

        if let Err(e) = &umount_output {
            warn!(agent_id = %agent.id, error = %e, "Failed to unmount rootfs");
        }

        result?;
        info!(agent_id = %agent.id, "Rootfs config injected");
        Ok(())
    }

    /// Write network and environment config files into a mounted rootfs.
    async fn write_rootfs_config(
        &self,
        agent: &Agent,
        config: &Config,
        alloc: &crate::tap_manager::TapAllocation,
        mount_dir: &str,
    ) -> AppResult<()> {
        let agent_id = agent.id.to_string();

        // Write network config
        let fc_network = format!(
            "GUEST_IP={}\nGUEST_CIDR={}/30\nGATEWAY_IP={}\n",
            alloc.guest_ip, alloc.guest_ip, alloc.host_ip
        );
        let tmp_net = format!("/tmp/fc-net-{}", &agent_id[..8]);
        tokio::fs::write(&tmp_net, &fc_network).await
            .map_err(|e| AppError::Internal(format!("Write network tmp: {e}")))?;
        let net_path = format!("{}/etc/fc-network", mount_dir);
        let cp_output = Command::new("sudo").args(["cp", &tmp_net, &net_path]).output().await
            .map_err(|e| AppError::Internal(format!("cp fc-network: {e}")))?;
        let _ = tokio::fs::remove_file(&tmp_net).await;
        if !cp_output.status.success() {
            let stderr = String::from_utf8_lossy(&cp_output.stderr);
            return Err(AppError::Internal(format!("cp fc-network failed: {stderr}")));
        }

        // Write environment config
        // Use proxy model with openrouter/ prefix for backend LLM proxy
        let proxy_model = if agent.model.starts_with("openrouter/") {
            agent.model.clone()
        } else {
            format!("openrouter/{}", agent.model)
        };

        // Build the OpenRouter API key that encodes internal auth
        let api_key = format!(
            "{}|{}|{}",
            config.internal_secret, agent.id, agent.user_id
        );

        // Backend proxy URL uses the host gateway IP
        let backend_url = format!("http://{}:8080", alloc.host_ip);

        let openclaw_env = format!(
            r#"export AGENT_NAME="agent-{agent_short}"
export AGENT_MODEL="{proxy_model}"
export AGENT_PORT="3000"
export OPENCLAW_GATEWAY_TOKEN="oneclick-internal"
export NODE_OPTIONS="--max-old-space-size=1280"
export OPENROUTER_BASE_URL="{backend_url}/internal/llm/v1"
export OPENROUTER_API_KEY="{api_key}"
export ONECLICK_BACKEND_URL="{backend_url}"
export ONECLICK_AGENT_ID="{agent_id}"
export ONECLICK_USER_ID="{user_id}"
export ONECLICK_INTERNAL_SECRET="{internal_secret}"
"#,
            agent_short = &agent.id.to_string()[..8],
            proxy_model = proxy_model,
            backend_url = backend_url,
            api_key = api_key,
            agent_id = agent.id,
            user_id = agent.user_id,
            internal_secret = config.internal_secret,
        );

        let tmp_env = format!("/tmp/fc-env-{}", &agent_id[..8]);
        tokio::fs::write(&tmp_env, &openclaw_env).await
            .map_err(|e| AppError::Internal(format!("Write env tmp: {e}")))?;
        let env_path = format!("{}/etc/openclaw-env", mount_dir);
        let cp_output = Command::new("sudo").args(["cp", &tmp_env, &env_path]).output().await
            .map_err(|e| AppError::Internal(format!("cp openclaw-env: {e}")))?;
        let _ = tokio::fs::remove_file(&tmp_env).await;
        if !cp_output.status.success() {
            let stderr = String::from_utf8_lossy(&cp_output.stderr);
            return Err(AppError::Internal(format!("cp openclaw-env failed: {stderr}")));
        }

        Ok(())
    }
}

#[async_trait]
impl AgentRuntime for FirecrackerRuntime {
    async fn create_agent(&self, agent: &Agent, config: &Config) -> AppResult<String> {
        let agent_id = agent.id.to_string();

        info!(
            agent_id = %agent.id,
            user_id = %agent.user_id,
            "Creating Firecracker VM"
        );

        // Create VM directory
        let vm_dir = self.vm_dir(&agent_id);
        tokio::fs::create_dir_all(&vm_dir)
            .await
            .map_err(|e| AppError::Internal(format!("Create VM dir: {e}")))?;

        // Allocate TAP device first — if this fails, only the empty dir leaks
        let alloc = self
            .tap_manager
            .allocate(&agent_id)
            .await
            .map_err(|e| {
                // Clean up the directory we just created
                let dir = vm_dir.clone();
                tokio::spawn(async move { let _ = tokio::fs::remove_dir_all(&dir).await; });
                AppError::Internal(e)
            })?;

        // Copy rootfs template (reflink if supported)
        let rootfs_dest = self.rootfs_path(&agent_id);
        let output = Command::new("cp")
            .arg("--reflink=auto")
            .arg(&self.config.fc_rootfs_template)
            .arg(&rootfs_dest)
            .output()
            .await
            .map_err(|e| AppError::Internal(format!("Copy rootfs: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.tap_manager.release(&agent_id).await;
            let _ = tokio::fs::remove_dir_all(&vm_dir).await;
            return Err(AppError::Internal(format!("Copy rootfs failed: {stderr}")));
        }

        // Inject per-VM configuration into the rootfs copy
        if let Err(err) = self.inject_rootfs_config(agent, config, &alloc).await {
            self.tap_manager.release(&agent_id).await;
            let _ = tokio::fs::remove_file(&rootfs_dest).await;
            let _ = tokio::fs::remove_dir_all(&vm_dir).await;
            return Err(err);
        }

        info!(
            agent_id = %agent.id,
            tap = %alloc.device,
            guest_ip = %alloc.guest_ip,
            "Firecracker VM prepared"
        );

        // Return the agent_id as the "container ID"
        Ok(format!("fc-{}", agent_id))
    }

    async fn start_agent(&self, container_id: &str) -> AppResult<()> {
        let agent_id = container_id.strip_prefix("fc-").unwrap_or(container_id);

        info!(agent_id, "Starting Firecracker VM");

        let alloc = self
            .tap_manager
            .get_allocation(agent_id)
            .await
            .ok_or_else(|| {
                AppError::Internal(format!("No TAP allocation for agent {agent_id}"))
            })?;

        // Extract snapshot and old VM from lock scope if available
        let restore_data = {
            let mut vms = self.vms.lock().await;
            if let Some(state) = vms.remove(agent_id) {
                if state.snapshot.is_some() {
                    Some(state)
                } else {
                    // No snapshot — put it back
                    vms.insert(agent_id.to_string(), state);
                    None
                }
            } else {
                None
            }
        };

        if let Some(mut vm_state) = restore_data {
            let snapshot = vm_state.snapshot.take().unwrap();
            // Restore without holding the lock
            match self.restore_from_snapshot(agent_id, snapshot, &mut vm_state.vm).await {
                Ok(new_vm) => {
                    vm_state.vm = new_vm;
                    vm_state.snapshot = None;
                    let mut vms = self.vms.lock().await;
                    vms.insert(agent_id.to_string(), vm_state);
                    info!(agent_id, "VM restored from in-memory snapshot");
                    return Ok(());
                }
                Err(e) => {
                    // Reinsert so we don't lose the VM state
                    warn!(agent_id, error = %e, "Snapshot restore failed, reinserting state");
                    let mut vms = self.vms.lock().await;
                    vms.insert(agent_id.to_string(), vm_state);
                    return Err(e);
                }
            }
        }

        // Check for on-disk snapshot
        let snap_path = self.snapshot_dir(agent_id).join("vm.snap");
        let mem_path = self.snapshot_dir(agent_id).join("vm.mem");

        if snap_path.exists() && mem_path.exists() {
            info!(agent_id, "Restoring from on-disk snapshot");
            warn!(agent_id, "On-disk snapshot found but no in-memory VmSnapshot — cold booting instead");
        }

        // Cold boot (no lock held)
        let vm = self.cold_boot(agent_id, &alloc.device, &alloc.guest_mac).await?;

        let mut vms = self.vms.lock().await;
        vms.insert(agent_id.to_string(), VmState {
            vm,
            tap_device: alloc.device.clone(),
            guest_ip: alloc.guest_ip.clone(),
            socket_path: self.socket_path(agent_id),
            snapshot: None,
            has_snapshot_on_disk: false,
        });

        Ok(())
    }

    async fn stop_agent(&self, container_id: &str) -> AppResult<()> {
        let agent_id = container_id.strip_prefix("fc-").unwrap_or(container_id);

        info!(agent_id, "Stopping Firecracker VM (snapshot sleep)");

        // Take VM out of map to avoid holding lock during snapshot
        let mut vm_state = {
            let mut vms = self.vms.lock().await;
            vms.remove(agent_id).ok_or_else(|| {
                AppError::Internal(format!("No running VM for agent {agent_id}"))
            })?
        };

        match self.snapshot_and_shutdown(agent_id, &mut vm_state.vm).await {
            Ok(snapshot) => {
                vm_state.snapshot = Some(snapshot);
                vm_state.has_snapshot_on_disk = true;
                let mut vms = self.vms.lock().await;
                vms.insert(agent_id.to_string(), vm_state);
                info!(agent_id, "VM stopped with snapshot");
                Ok(())
            }
            Err(e) => {
                // Reinsert so the VM isn't lost from bookkeeping
                warn!(agent_id, error = %e, "Snapshot failed, reinserting VM state");
                let mut vms = self.vms.lock().await;
                vms.insert(agent_id.to_string(), vm_state);
                Err(e)
            }
        }
    }

    async fn destroy_agent(&self, container_id: &str) -> AppResult<()> {
        let agent_id = container_id.strip_prefix("fc-").unwrap_or(container_id);

        info!(agent_id, "Destroying Firecracker VM");

        // Shut down VM if running
        let mut vms = self.vms.lock().await;
        if let Some(mut state) = vms.remove(agent_id) {
            if let Err(e) = self.shutdown_vm(&mut state.vm).await {
                warn!(agent_id, error = %e, "VM shutdown failed during destroy");
            }
        }
        drop(vms);

        // Release TAP device
        self.tap_manager.release(agent_id).await;

        // Clean up files
        let vm_dir = self.vm_dir(agent_id);
        let snap_dir = self.snapshot_dir(agent_id);
        let socket = self.socket_path(agent_id);

        let _ = tokio::fs::remove_dir_all(&vm_dir).await;
        let _ = tokio::fs::remove_dir_all(&snap_dir).await;
        let _ = tokio::fs::remove_file(&socket).await;

        info!(agent_id, "Firecracker VM destroyed");
        Ok(())
    }

    async fn health_check(&self, container_id: &str) -> AppResult<bool> {
        let agent_id = container_id.strip_prefix("fc-").unwrap_or(container_id);

        let guest_ip = {
            let vms = self.vms.lock().await;
            match vms.get(agent_id) {
                Some(s) => s.guest_ip.clone(),
                None => return Ok(false),
            }
        };

        let healthy = Self::probe_health(&guest_ip, AGENT_BRIDGE_PORT).await;
        Ok(healthy)
    }

    async fn get_host_port(&self, _container_id: &str) -> AppResult<Option<u16>> {
        // Firecracker VMs don't use host port mapping — traffic goes
        // directly to the guest IP via the TAP network.
        Ok(None)
    }

    async fn get_agent_address(&self, container_id: &str) -> AppResult<String> {
        let agent_id = container_id.strip_prefix("fc-").unwrap_or(container_id);

        {
            let vms = self.vms.lock().await;
            if let Some(state) = vms.get(agent_id) {
                return Ok(state.guest_ip.clone());
            }
        }

        // Fallback: get from TAP allocation (lock already dropped)
        if let Some(alloc) = self.tap_manager.get_allocation(agent_id).await {
            return Ok(alloc.guest_ip);
        }

        Err(AppError::Internal(format!(
            "No address for Firecracker agent {agent_id}"
        )))
    }

    fn agent_name(&self, _user_id: &Uuid, agent_id: &Uuid) -> String {
        format!("fc-{}", agent_id)
    }

    fn health_check_budget(&self) -> (u32, Duration) {
        // Firecracker cold boot: 60 × 1s = 60s
        // Snapshot restore: much faster, but we use the same budget
        // (the orchestrator will pass quickly on healthy VMs)
        (60, Duration::from_secs(1))
    }
}
