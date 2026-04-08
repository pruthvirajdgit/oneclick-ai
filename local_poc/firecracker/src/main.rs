use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command;

// fctools imports — the official Firecracker Rust SDK
use fctools::{
    process_spawner::SudoProcessSpawner,
    runtime::tokio::TokioRuntime,
    vm::{
        Vm,
        api::VmApi,
        configuration::{InitMethod, VmConfiguration, VmConfigurationData},
        models::{
            BootSource, CreateSnapshot, Drive,
            MachineConfiguration,
            NetworkInterface, SnapshotType,
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

// Type aliases for our concrete fctools VM type
type FcVm = Vm<UnrestrictedVmmExecutor, SudoProcessSpawner, TokioRuntime>;

const VM_IP: &str = "172.16.0.2";
const KERNEL_PATH: &str = "resources/vmlinux-6.1";
const SOCKET_PATH: &str = "/tmp/fc-poc.socket";
const LOG_PATH: &str = "/tmp/fc-poc.log";
const PID_FILE: &str = "/tmp/fc-poc.pid";
const TAP_DEV: &str = "tap0";
const GUEST_MAC: &str = "AA:FC:00:00:00:01";
const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/fc-init";
const FC_BIN: &str = "/usr/local/bin/firecracker";
const JAILER_BIN: &str = "/usr/local/bin/jailer";
const SNAP_EDITOR_BIN: &str = "/usr/local/bin/snapshot-editor";
const STATE_FILE: &str = "/tmp/fc-poc-state.json";

// Stage 1 (basic) defaults
const BASIC_ROOTFS: &str = "resources/rootfs.ext4";
const BASIC_PORT: u16 = 8080;
const BASIC_MEM_MIB: usize = 256;
const BASIC_HEALTH_PATH: &str = "/health";
const BASIC_BOOT_TIMEOUT: u64 = 30;

// Stage 2 (openclaw) defaults
const OPENCLAW_ROOTFS: &str = "resources/rootfs-openclaw.ext4";
const _OPENCLAW_GATEWAY_PORT: u16 = 3000;
const OPENCLAW_BRIDGE_PORT: u16 = 3001;
const OPENCLAW_MEM_MIB: usize = 1536;
const OPENCLAW_HEALTH_PATH: &str = "/health";
const OPENCLAW_BOOT_TIMEOUT: u64 = 600;

#[derive(Copy, Clone, ValueEnum, Debug)]
enum Profile {
    /// Stage 1: minimal rootfs with busybox httpd
    Basic,
    /// Stage 2: OpenClaw gateway + chat bridge
    Openclaw,
}

struct ProfileConfig {
    rootfs: &'static str,
    health_port: u16,
    health_path: &'static str,
    mem_mib: usize,
    boot_timeout: u64,
    snapshot_dir: &'static str,
}

impl Profile {
    fn config(&self) -> ProfileConfig {
        match self {
            Profile::Basic => ProfileConfig {
                rootfs: BASIC_ROOTFS,
                health_port: BASIC_PORT,
                health_path: BASIC_HEALTH_PATH,
                mem_mib: BASIC_MEM_MIB,
                boot_timeout: BASIC_BOOT_TIMEOUT,
                snapshot_dir: "snapshots",
            },
            Profile::Openclaw => ProfileConfig {
                rootfs: OPENCLAW_ROOTFS,
                health_port: OPENCLAW_BRIDGE_PORT,
                health_path: OPENCLAW_HEALTH_PATH,
                mem_mib: OPENCLAW_MEM_MIB,
                boot_timeout: OPENCLAW_BOOT_TIMEOUT,
                snapshot_dir: "snapshots-openclaw",
            },
        }
    }
}

/// Persisted state between CLI invocations
#[derive(Serialize, Deserialize)]
struct VmState {
    pid: u32,
    socket_path: String,
    snapshot_dir: String,
    snapshot_exists: bool,
}

#[derive(Parser)]
#[command(name = "firecracker-poc", about = "Firecracker MicroVM PoC CLI (fctools-based)")]
struct Cli {
    /// VM profile (basic = Stage 1, openclaw = Stage 2)
    #[arg(long, value_enum, default_value_t = Profile::Basic)]
    profile: Profile,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create and start VM (combined, uses fctools Vm layer)
    Create,
    /// Start the VM (alias for create — creates + boots in one step)
    Start,
    /// Health check — verify HTTP server responds
    Check,
    /// Snapshot the VM to disk and kill the Firecracker process
    Stop,
    /// Restore VM from snapshot (uses fctools VmConfiguration::RestoredFromSnapshot)
    Wake,
    /// Kill the Firecracker process without snapshotting
    Destroy,
    /// Full lifecycle: create → start → check → stop → wake → check (all fctools, one process)
    Lifecycle,
    /// Run 5 consecutive stop/wake cycles (all fctools, one process)
    Stress,
    /// Send a chat message via the bridge (openclaw profile only)
    Chat {
        /// The message to send
        #[arg(default_value = "Hello! Please respond with a short greeting.")]
        message: String,
    },
}

// ─── Standalone VM management (for cross-process commands) ───────────────────
// These functions spawn/manage the FC process directly and use raw HTTP for API
// calls. Each HTTP call is a fresh connection — no persistent hyper client that
// would block FC's API server after the CLI process exits.

/// Serializable types for raw Firecracker API calls
#[derive(Serialize)]
struct RawBootSource {
    kernel_image_path: String,
    boot_args: String,
}
#[derive(Serialize)]
struct RawDrive {
    drive_id: String,
    path_on_host: String,
    is_root_device: bool,
    is_read_only: bool,
}
#[derive(Serialize)]
struct RawMachineConfig {
    vcpu_count: u32,
    mem_size_mib: usize,
    track_dirty_pages: bool,
}
#[derive(Serialize)]
struct RawNetworkInterface {
    iface_id: String,
    guest_mac: String,
    host_dev_name: String,
}
#[derive(Serialize)]
struct RawAction {
    action_type: String,
}
#[derive(Serialize)]
struct RawSnapshotLoad {
    snapshot_path: String,
    mem_backend: RawMemBackend,
    enable_diff_snapshots: bool,
    resume_vm: bool,
}
#[derive(Serialize)]
struct RawMemBackend {
    backend_type: String,
    backend_path: String,
}

fn abs_path_str(relative: &str) -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(relative)
        .to_string_lossy()
        .to_string()
}

/// Send an HTTP request to the Firecracker API via Unix socket (fresh connection each time)
async fn fc_request(
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<(u16, String)> {
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::body::Incoming;
    use hyper::Request;
    use hyper_util::rt::TokioIo;
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(SOCKET_PATH)
        .await
        .context("Failed to connect to Firecracker socket")?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .context("HTTP handshake failed")?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("Connection error: {}", e);
        }
    });

    let body_bytes = body.unwrap_or_default();
    let req = Request::builder()
        .method(method)
        .uri(format!("http://localhost{}", path))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .context("Failed to build request")?;

    let resp: hyper::Response<Incoming> = sender
        .send_request(req)
        .await
        .context("Failed to send request")?;

    let status = resp.status().as_u16();
    let body = resp.into_body().collect().await
        .context("Failed to read response body")?
        .to_bytes();
    Ok((status, String::from_utf8_lossy(&body).to_string()))
}

/// Start a new Firecracker process directly (not via fctools)
async fn start_firecracker_process() -> Result<u32> {
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;
    // Remove old log (may be root-owned from previous sudo run)
    let _ = Command::new("sudo")
        .args(["rm", "-f", LOG_PATH])
        .output()
        .await;
    tokio::fs::write(LOG_PATH, "").await?;

    let child = Command::new("sudo")
        .arg("firecracker")
        .arg("--api-sock")
        .arg(SOCKET_PATH)
        .arg("--log-path")
        .arg(LOG_PATH)
        .arg("--level")
        .arg("Info")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to start firecracker process")?;

    let pid = child.id().unwrap_or(0);

    for _ in 0..50 {
        if std::path::Path::new(SOCKET_PATH).exists() {
            let _ = Command::new("sudo")
                .args(["chmod", "666", SOCKET_PATH])
                .output()
                .await;
            tokio::fs::write(PID_FILE, pid.to_string()).await?;
            return Ok(pid);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("Firecracker socket did not appear after 5s");
}

/// Configure VM via raw Firecracker API calls
async fn configure_vm(cfg: &ProfileConfig) -> Result<()> {
    let boot = RawBootSource {
        kernel_image_path: abs_path_str(KERNEL_PATH),
        boot_args: BOOT_ARGS.into(),
    };
    let (status, body) = fc_request("PUT", "/boot-source", Some(serde_json::to_string(&boot)?)).await?;
    if status != 204 { bail!("boot-source failed: {} {}", status, body); }

    let drive = RawDrive {
        drive_id: "rootfs".into(),
        path_on_host: abs_path_str(cfg.rootfs),
        is_root_device: true,
        is_read_only: false,
    };
    let (status, body) = fc_request("PUT", "/drives/rootfs", Some(serde_json::to_string(&drive)?)).await?;
    if status != 204 { bail!("drives failed: {} {}", status, body); }

    let config = RawMachineConfig {
        vcpu_count: 2,
        mem_size_mib: cfg.mem_mib,
        track_dirty_pages: true,
    };
    let (status, body) = fc_request("PUT", "/machine-config", Some(serde_json::to_string(&config)?)).await?;
    if status != 204 { bail!("machine-config failed: {} {}", status, body); }

    let net = RawNetworkInterface {
        iface_id: "eth0".into(),
        guest_mac: GUEST_MAC.into(),
        host_dev_name: TAP_DEV.into(),
    };
    let (status, body) = fc_request("PUT", "/network-interfaces/eth0", Some(serde_json::to_string(&net)?)).await?;
    if status != 204 { bail!("network-interfaces failed: {} {}", status, body); }

    Ok(())
}

/// Send InstanceStart action via raw API
async fn instance_start() -> Result<()> {
    let action = RawAction { action_type: "InstanceStart".into() };
    let (status, body) = fc_request("PUT", "/actions", Some(serde_json::to_string(&action)?)).await?;
    if status != 204 { bail!("InstanceStart failed: {} {}", status, body); }
    Ok(())
}

/// Kill the Firecracker process by PID
async fn kill_firecracker() -> Result<()> {
    if let Ok(pid_str) = tokio::fs::read_to_string(PID_FILE).await {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            let _ = Command::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg(format!("kill {} 2>/dev/null; sleep 1; kill -9 {} 2>/dev/null", pid, pid))
                .output()
                .await;
            let _ = Command::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg("kill -9 $(pgrep -f 'firecracker --api-sock /tmp/fc-poc') 2>/dev/null")
                .output()
                .await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    let _ = tokio::fs::remove_file(PID_FILE).await;
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;
    Ok(())
}

/// Standalone snapshot sleep: pause → snapshot → kill (raw HTTP, cross-process safe)
async fn standalone_snapshot_sleep(snapshot_dir: &str) -> Result<()> {
    let snap_state = format!("{}/vm.snap", snapshot_dir);
    let snap_mem = format!("{}/vm.mem", snapshot_dir);
    tokio::fs::create_dir_all(snapshot_dir).await?;

    let (status, body) = fc_request("PATCH", "/vm", Some(r#"{"state": "Paused"}"#.into())).await?;
    if status != 204 { bail!("Pause failed: {} {}", status, body); }
    println!("  VM paused");

    let snap_body = serde_json::json!({
        "snapshot_type": "Full",
        "snapshot_path": abs_path_str(&snap_state),
        "mem_file_path": abs_path_str(&snap_mem),
    });
    let (status, body) = fc_request("PUT", "/snapshot/create", Some(snap_body.to_string())).await?;
    if status != 204 { bail!("CreateSnapshot failed: {} {}", status, body); }
    println!("  Snapshot saved to {}", snapshot_dir);

    kill_firecracker().await?;
    println!("  Firecracker process killed");
    Ok(())
}

/// Standalone snapshot wake: new FC process → load snapshot → resume (raw HTTP)
async fn standalone_snapshot_wake(snapshot_dir: &str) -> Result<Duration> {
    let snap_state = format!("{}/vm.snap", snapshot_dir);
    let snap_mem = format!("{}/vm.mem", snapshot_dir);
    let start = Instant::now();

    // Clean up stale socket before starting new FC process
    let _ = Command::new("sudo").args(["rm", "-f", SOCKET_PATH]).output().await;

    let pid = start_firecracker_process().await?;
    println!("  New Firecracker process: PID {}", pid);

    // Wait for the FC API socket to become ready
    for i in 0..50 {
        if tokio::net::UnixStream::connect(SOCKET_PATH).await.is_ok() {
            break;
        }
        if i == 49 {
            // Socket never became ready — kill the orphaned process
            let _ = Command::new("sudo").args(["kill", &pid.to_string()]).output().await;
            bail!("Firecracker socket not ready after 5s");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let load = RawSnapshotLoad {
        snapshot_path: abs_path_str(&snap_state),
        mem_backend: RawMemBackend {
            backend_type: "File".into(),
            backend_path: abs_path_str(&snap_mem),
        },
        enable_diff_snapshots: false,
        resume_vm: true,
    };
    let result = fc_request("PUT", "/snapshot/load", Some(serde_json::to_string(&load)?)).await;
    match result {
        Ok((status, _body)) if status == 204 => {},
        Ok((status, body)) => {
            let _ = Command::new("sudo").args(["kill", &pid.to_string()]).output().await;
            bail!("LoadSnapshot failed: {} {}", status, body);
        }
        Err(e) => {
            let _ = Command::new("sudo").args(["kill", &pid.to_string()]).output().await;
            return Err(e);
        }
    }

    Ok(start.elapsed())
}

// ─── fctools helpers (for in-process lifecycle/stress commands) ───────────────

fn abs_path(relative: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(relative)
}

fn fc_installation() -> VmmInstallation {
    VmmInstallation::new(FC_BIN, JAILER_BIN, SNAP_EDITOR_BIN)
}

fn fc_spawner() -> SudoProcessSpawner {
    SudoProcessSpawner::new(None, None)
}

fn fc_executor() -> UnrestrictedVmmExecutor {
    let args = VmmArguments::new(VmmApiSocket::Enabled(SOCKET_PATH.into()));
    UnrestrictedVmmExecutor::new(args)
}

fn fc_resource_system() -> ResourceSystem<SudoProcessSpawner, TokioRuntime> {
    ResourceSystem::new(fc_spawner(), TokioRuntime, VmmOwnershipModel::UpgradedPermanently)
}

/// Build the VmConfigurationData for a new or restored VM
fn build_vm_config(
    cfg: &ProfileConfig,
    resource_system: &mut ResourceSystem<SudoProcessSpawner, TokioRuntime>,
) -> Result<VmConfigurationData> {
    let kernel = resource_system
        .create_resource(abs_path(KERNEL_PATH), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
        .map_err(|e| anyhow::anyhow!("Failed to create kernel resource: {:?}", e))?;

    let rootfs = resource_system
        .create_resource(abs_path(cfg.rootfs), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
        .map_err(|e| anyhow::anyhow!("Failed to create rootfs resource: {:?}", e))?;

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
            vcpu_count: 2,
            mem_size_mib: cfg.mem_mib,
            smt: None,
            track_dirty_pages: Some(true),
            huge_pages: None,
        },
        cpu_template: None,
        network_interfaces: vec![NetworkInterface {
            iface_id: "eth0".into(),
            host_dev_name: TAP_DEV.into(),
            guest_mac: Some(GUEST_MAC.into()),
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

/// Create and start a new VM using fctools (cold boot)
async fn create_and_start_vm(cfg: &ProfileConfig) -> Result<FcVm> {
    // Clean up old socket
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;

    let mut resource_system = fc_resource_system();
    let data = build_vm_config(cfg, &mut resource_system)?;

    let configuration = VmConfiguration::New {
        init_method: InitMethod::ViaApiCalls,
        data,
    };

    let mut vm = Vm::prepare(fc_executor(), resource_system, fc_installation(), configuration)
        .await
        .map_err(|e| anyhow::anyhow!("Vm::prepare failed: {:?}", e))?;

    // Spawn background task to chmod the socket as soon as FC creates it.
    // fctools' socket wait loop connects as non-root, but FC (via sudo) creates
    // a root-owned socket. We fix permissions so the wait loop succeeds.
    let socket_chmod_handle = tokio::spawn(async {
        for _ in 0..100 {
            if std::path::Path::new(SOCKET_PATH).exists() {
                let _ = Command::new("sudo")
                    .args(["chmod", "666", SOCKET_PATH])
                    .output()
                    .await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    println!("  Starting VM via fctools...");
    vm.start(Duration::from_secs(30))
        .await
        .map_err(|e| anyhow::anyhow!("Vm::start failed: {:?}", e))?;

    socket_chmod_handle.abort(); // cleanup if still running
    println!("  VM started (fctools managed)");
    Ok(vm)
}

/// Create a snapshot from a running VM (pause → snapshot → shutdown)
/// Returns the VmSnapshot for later restoration.
async fn snapshot_sleep_fctools(
    vm: &mut FcVm,
    cfg: &ProfileConfig,
) -> Result<VmSnapshot> {
    tokio::fs::create_dir_all(cfg.snapshot_dir).await?;

    let snap_path = abs_path(&format!("{}/vm.snap", cfg.snapshot_dir));
    let mem_path = abs_path(&format!("{}/vm.mem", cfg.snapshot_dir));

    // Pause via fctools VmApi
    vm.pause()
        .await
        .map_err(|e| anyhow::anyhow!("Pause failed: {:?}", e))?;
    println!("  VM paused (fctools)");

    // Create snapshot resources (Produced = output files created by Firecracker)
    let snap_resource = vm.get_resource_system_mut()
        .create_resource(&snap_path, ResourceType::Produced)
        .map_err(|e| anyhow::anyhow!("Failed to create snapshot resource: {:?}", e))?;
    let mem_resource = vm.get_resource_system_mut()
        .create_resource(&mem_path, ResourceType::Produced)
        .map_err(|e| anyhow::anyhow!("Failed to create mem resource: {:?}", e))?;

    let create_snap = CreateSnapshot {
        snapshot_type: Some(SnapshotType::Full),
        snapshot: snap_resource,
        mem_file: mem_resource,
    };

    let snapshot = vm.create_snapshot(create_snap)
        .await
        .map_err(|e| anyhow::anyhow!("CreateSnapshot failed: {:?}", e))?;
    println!("  Snapshot saved to {} (fctools)", cfg.snapshot_dir);

    Ok(snapshot)
}

/// Restore VM from snapshot using fctools VmSnapshot::prepare_vm
async fn snapshot_wake_fctools(
    snapshot: VmSnapshot,
    old_vm: &mut FcVm,
    _cfg: &ProfileConfig,
) -> Result<(FcVm, Duration)> {
    let start = Instant::now();

    // Clean up old socket for new FC process
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;

    let options = PrepareVmFromSnapshotOptions {
        executor: fc_executor(),
        process_spawner: fc_spawner(),
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
        .map_err(|e| anyhow::anyhow!("prepare_vm failed: {:?}", e))?;

    // chmod socket for non-root access
    let socket_chmod_handle = tokio::spawn(async {
        for _ in 0..100 {
            if std::path::Path::new(SOCKET_PATH).exists() {
                let _ = Command::new("sudo")
                    .args(["chmod", "666", SOCKET_PATH])
                    .output()
                    .await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    new_vm.start(Duration::from_secs(10))
        .await
        .map_err(|e| anyhow::anyhow!("Vm::start (restore) failed: {:?}", e))?;

    socket_chmod_handle.abort();
    let restore_time = start.elapsed();
    println!("  Snapshot restored in {:.0}ms (fctools)", restore_time.as_millis());

    Ok((new_vm, restore_time))
}

/// Shutdown a VM using fctools
async fn shutdown_vm(vm: &mut FcVm) -> Result<()> {
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
        .map_err(|e| anyhow::anyhow!("Shutdown failed: {:?}", e))?;
    println!("  VM shut down (fctools)");
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;
    Ok(())
}

// ─── Health check (TCP, no FC API needed) ────────────────────────────────────

async fn health_check(port: u16, path: &str) -> Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("{}:{}", VM_IP, port);
    let stream = match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    ).await {
        Ok(Ok(s)) => s,
        _ => return Ok(false),
    };

    let (mut reader, mut writer) = stream.into_split();
    let req = format!("GET {} HTTP/1.0\r\nHost: {}\r\n\r\n", path, addr);
    if writer.write_all(req.as_bytes()).await.is_err() {
        return Ok(false);
    }

    let mut buf = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(3), reader.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => {
            let resp = String::from_utf8_lossy(&buf[..n]);
            Ok(resp.contains("200") || resp.contains("ok"))
        }
        _ => Ok(false),
    }
}

async fn wait_for_health(timeout_secs: u64, port: u16, path: &str) -> Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    while Instant::now() < deadline {
        if health_check(port, path).await.unwrap_or(false) {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(false)
}

// ─── State persistence ───────────────────────────────────────────────────────

async fn save_state(state: &VmState) -> Result<()> {
    tokio::fs::write(STATE_FILE, serde_json::to_string_pretty(state)?).await?;
    Ok(())
}

// ─── CLI Commands ────────────────────────────────────────────────────────────

/// Create + start VM using standalone process management (cross-process safe)
async fn cmd_create_start(cfg: &ProfileConfig) -> Result<()> {
    println!("Creating and starting VM...");
    let start = Instant::now();

    // Clean up stale socket
    let _ = Command::new("sudo").args(["rm", "-f", SOCKET_PATH]).output().await;

    let pid = start_firecracker_process().await?;
    println!("  Firecracker process started: PID {}", pid);

    if let Err(e) = configure_vm(cfg).await {
        let _ = Command::new("sudo").args(["kill", &pid.to_string()]).output().await;
        return Err(e);
    }
    println!("  VM configured (kernel={}, rootfs={}, mem={}MB)", KERNEL_PATH, cfg.rootfs, cfg.mem_mib);

    if let Err(e) = instance_start().await {
        let _ = Command::new("sudo").args(["kill", &pid.to_string()]).output().await;
        return Err(e);
    }
    println!("  InstanceStart sent ({:.0}ms)", start.elapsed().as_millis());

    println!("  Waiting for health check on port {}{}...", cfg.health_port, cfg.health_path);
    if wait_for_health(cfg.boot_timeout, cfg.health_port, cfg.health_path).await? {
        println!("VM started and healthy ({:.1}s total)", start.elapsed().as_secs_f64());
    } else {
        bail!("VM started but health check failed after {}s", cfg.boot_timeout);
    }

    Ok(())
}

async fn cmd_check(cfg: &ProfileConfig) -> Result<()> {
    print!("Health check (port {}{})... ", cfg.health_port, cfg.health_path);
    let start = Instant::now();
    if health_check(cfg.health_port, cfg.health_path).await? {
        println!("OK ({:.0}ms)", start.elapsed().as_millis());
    } else {
        println!("FAILED");
        bail!("Health check failed — server not responding on port {}", cfg.health_port);
    }
    Ok(())
}

/// Stop: cross-process fallback (HTTP for pause+snapshot, then kill)
async fn cmd_stop(cfg: &ProfileConfig) -> Result<()> {
    println!("Stopping VM (snapshot sleep)...");
    standalone_snapshot_sleep(cfg.snapshot_dir).await?;
    save_state(&VmState {
        pid: 0,
        socket_path: SOCKET_PATH.into(),
        snapshot_dir: cfg.snapshot_dir.into(),
        snapshot_exists: true,
    }).await?;
    println!("VM stopped. Snapshot saved.");
    Ok(())
}

/// Wake: standalone snapshot restore (raw HTTP, cross-process safe)
async fn cmd_wake(cfg: &ProfileConfig) -> Result<()> {
    println!("Waking VM (snapshot restore)...");
    let restore_time = standalone_snapshot_wake(cfg.snapshot_dir).await?;
    println!("  Snapshot restored in {:.0}ms", restore_time.as_millis());

    println!("  Waiting for health check...");
    let start = Instant::now();
    if wait_for_health(10, cfg.health_port, cfg.health_path).await? {
        println!("VM awake and healthy ({:.0}ms after restore)", start.elapsed().as_millis());
    } else {
        bail!("VM restored but health check failed after 10s");
    }
    Ok(())
}

async fn cmd_destroy(cfg: &ProfileConfig) -> Result<()> {
    println!("Destroying VM...");
    kill_firecracker().await?;
    let snap_state = format!("{}/vm.snap", cfg.snapshot_dir);
    let snap_mem = format!("{}/vm.mem", cfg.snapshot_dir);
    let _ = tokio::fs::remove_file(&snap_state).await;
    let _ = tokio::fs::remove_file(&snap_mem).await;
    let _ = tokio::fs::remove_file(STATE_FILE).await;
    println!("VM destroyed.");
    Ok(())
}

/// Full lifecycle in one process — 100% fctools
async fn cmd_lifecycle(cfg: &ProfileConfig) -> Result<()> {
    println!("=== Full lifecycle test (fctools end-to-end) ===\n");

    // Step 1: Create + start
    println!("--- Step 1: Create and start VM ---");
    let start = Instant::now();
    let mut vm = create_and_start_vm(cfg).await?;

    println!("  Waiting for health check...");
    if wait_for_health(cfg.boot_timeout, cfg.health_port, cfg.health_path).await? {
        println!("  ✓ VM healthy ({:.1}s)", start.elapsed().as_secs_f64());
    } else {
        bail!("Health check failed after cold boot");
    }

    // Step 2: Health check
    println!("\n--- Step 2: Health check ---");
    let start = Instant::now();
    if health_check(cfg.health_port, cfg.health_path).await? {
        println!("  ✓ Health OK ({:.0}ms)", start.elapsed().as_millis());
    } else {
        bail!("Health check failed");
    }

    // Step 3: Snapshot sleep (pause → snapshot → shutdown old VM to release TAP)
    println!("\n--- Step 3: Snapshot sleep ---");
    let snapshot = snapshot_sleep_fctools(&mut vm, cfg).await?;
    shutdown_vm(&mut vm).await?;
    println!("  Old VM shutdown, TAP released");

    // Step 4: Snapshot wake (restore via prepare_vm)
    println!("\n--- Step 4: Snapshot wake ---");
    let (mut new_vm, restore_time) = snapshot_wake_fctools(snapshot, &mut vm, cfg).await?;

    println!("  Waiting for health check...");
    let start = Instant::now();
    if wait_for_health(10, cfg.health_port, cfg.health_path).await? {
        println!("  ✓ VM healthy ({:.0}ms after restore)", start.elapsed().as_millis());
    } else {
        bail!("Health check failed after snapshot restore");
    }

    // Step 5: Final health check
    println!("\n--- Step 5: Final health check ---");
    let start = Instant::now();
    if health_check(cfg.health_port, cfg.health_path).await? {
        println!("  ✓ Health OK ({:.0}ms)", start.elapsed().as_millis());
    } else {
        bail!("Final health check failed");
    }

    // Cleanup
    shutdown_vm(&mut new_vm).await?;
    println!("\n=== Lifecycle test PASSED ===");
    println!("  Restore time: {:.0}ms", restore_time.as_millis());
    Ok(())
}

/// 5-cycle stress test — 100% fctools
async fn cmd_stress(cfg: &ProfileConfig) -> Result<()> {
    println!("=== 5-cycle stress test (fctools end-to-end) ===\n");

    // Initial cold boot
    println!("--- Initial boot ---");
    let mut vm = create_and_start_vm(cfg).await?;
    if !wait_for_health(cfg.boot_timeout, cfg.health_port, cfg.health_path).await? {
        bail!("Initial boot health check failed");
    }
    println!("  ✓ VM healthy\n");

    let mut restore_times = Vec::new();

    for i in 1..=5 {
        println!("--- Cycle {}/5 ---", i);

        // Snapshot sleep
        println!("  Stopping (snapshot via fctools)...");
        let snapshot = snapshot_sleep_fctools(&mut vm, cfg).await?;
        shutdown_vm(&mut vm).await?;

        // Restore from snapshot
        println!("  Waking (restore via fctools)...");
        let (new_vm, restore_time) = snapshot_wake_fctools(snapshot, &mut vm, cfg).await?;
        vm = new_vm;

        let ms = restore_time.as_millis();
        restore_times.push(ms);
        println!("  Restored in {}ms", ms);

        // Health check
        print!("  Health check... ");
        let start = Instant::now();
        if wait_for_health(10, cfg.health_port, cfg.health_path).await? {
            println!("OK ({:.0}ms)", start.elapsed().as_millis());
        } else {
            bail!("Health check failed on cycle {}", i);
        }
        println!();
    }

    // Cleanup
    shutdown_vm(&mut vm).await?;

    println!("=== Results ===");
    for (i, ms) in restore_times.iter().enumerate() {
        println!("  Cycle {}: {}ms", i + 1, ms);
    }
    let avg: u128 = restore_times.iter().sum::<u128>() / restore_times.len() as u128;
    let max = restore_times.iter().max().unwrap_or(&0);
    println!("  Average: {}ms", avg);
    println!("  Max: {}ms", max);
    if *max <= 500 {
        println!("  ✓ All restores under 500ms target!");
    } else {
        println!("  ✗ Some restores exceeded 500ms target");
    }
    Ok(())
}

/// Send a chat message via the bridge (Stage 2 only)
async fn cmd_chat(message: &str) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    println!("Sending chat message via bridge (port {})...", OPENCLAW_BRIDGE_PORT);
    println!("  Message: {}", message);

    let addr = format!("{}:{}", VM_IP, OPENCLAW_BRIDGE_PORT);
    let body = serde_json::json!({ "message": message }).to_string();

    let stream = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .context("Connection timeout")?
    .context("Failed to connect to bridge")?;

    let (mut reader, mut writer) = stream.into_split();

    let req = format!(
        "POST /chat HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        addr, body.len(), body
    );
    writer.write_all(req.as_bytes()).await?;

    println!("\n--- Response (SSE stream) ---");
    let mut buf = vec![0u8; 8192];
    let mut accumulated = String::new();
    let deadline = Instant::now() + Duration::from_secs(120);

    loop {
        if Instant::now() > deadline {
            println!("\n[Timeout after 120s]");
            break;
        }

        match tokio::time::timeout(Duration::from_secs(30), reader.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                let chunk = String::from_utf8_lossy(&buf[..n]);
                accumulated.push_str(&chunk);

                while let Some(data_start) = accumulated.find("data: ") {
                    let rest = &accumulated[data_start + 6..];
                    if let Some(end) = rest.find('\n') {
                        let data_line = &rest[..end].trim();
                        if *data_line == "[DONE]" {
                            println!("\n--- Chat complete ---");
                            return Ok(());
                        }
                        if let Ok(evt) = serde_json::from_str::<serde_json::Value>(data_line) {
                            if let Some(content) = evt.get("content").and_then(|c| c.as_str()) {
                                print!("{}", content);
                            }
                            if let Some(msg) = evt.get("message").and_then(|m| m.as_str()) {
                                if evt.get("type").and_then(|t| t.as_str()) == Some("error") {
                                    println!("\n[Error: {}]", msg);
                                }
                            }
                        }
                        accumulated = accumulated[data_start + 6 + end + 1..].to_string();
                    } else {
                        break;
                    }
                }
            }
            Ok(Err(e)) => bail!("Read error: {}", e),
            Err(_) => {
                println!("\n[No data for 30s, ending]");
                break;
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = cli.profile.config();

    println!("[profile: {:?}, fctools v0.7.0-alpha.1]", cli.profile);

    match cli.command {
        Commands::Create | Commands::Start => cmd_create_start(&cfg).await,
        Commands::Check => cmd_check(&cfg).await,
        Commands::Stop => cmd_stop(&cfg).await,
        Commands::Wake => cmd_wake(&cfg).await,
        Commands::Destroy => cmd_destroy(&cfg).await,
        Commands::Lifecycle => cmd_lifecycle(&cfg).await,
        Commands::Stress => cmd_stress(&cfg).await,
        Commands::Chat { message } => cmd_chat(&message).await,
    }
}
