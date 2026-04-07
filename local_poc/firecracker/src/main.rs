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
            BootSource, CreateSnapshot, Drive, LoadSnapshot,
            MachineConfiguration, MemoryBackend, MemoryBackendType,
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

// ─── fctools helpers ─────────────────────────────────────────────────────────

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
    ResourceSystem::new(fc_spawner(), TokioRuntime, VmmOwnershipModel::Shared)
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

    println!("  Starting VM via fctools...");
    vm.start(Duration::from_secs(30))
        .await
        .map_err(|e| anyhow::anyhow!("Vm::start failed: {:?}", e))?;

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
        ownership_model: VmmOwnershipModel::Shared,
        track_dirty_pages: Some(true),
        resume_vm: Some(true),
        network_overrides: vec![],
    };

    let mut new_vm = snapshot
        .prepare_vm(old_vm, options)
        .await
        .map_err(|e| anyhow::anyhow!("prepare_vm failed: {:?}", e))?;

    new_vm.start(Duration::from_secs(10))
        .await
        .map_err(|e| anyhow::anyhow!("Vm::start (restore) failed: {:?}", e))?;

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

// ─── Standalone command helpers (for cross-process stop/wake) ────────────────

/// Minimal HTTP over Unix socket — only used by standalone `stop` command.
/// In production, the Vm struct lives in the backend process, so this isn't needed.
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
            // Also kill any stray firecracker processes for our socket
            let _ = Command::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg("pkill -f 'firecracker --api-sock /tmp/fc-poc' 2>/dev/null")
                .output()
                .await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    let _ = tokio::fs::remove_file(PID_FILE).await;
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;
    Ok(())
}

/// Standalone stop: talks to existing VM via HTTP (cross-process fallback)
async fn standalone_snapshot_sleep(snapshot_dir: &str) -> Result<()> {
    let snap_state = format!("{}/vm.snap", snapshot_dir);
    let snap_mem = format!("{}/vm.mem", snapshot_dir);
    tokio::fs::create_dir_all(snapshot_dir).await?;

    // Pause
    let (status, body) = fc_request("PATCH", "/vm", Some(r#"{"state": "Paused"}"#.into())).await?;
    if status != 204 { bail!("Pause failed: {} {}", status, body); }
    println!("  VM paused");

    // Snapshot
    let snap_body = serde_json::json!({
        "snapshot_type": "Full",
        "snapshot_path": abs_path(&snap_state),
        "mem_file_path": abs_path(&snap_mem),
    });
    let (status, body) = fc_request("PUT", "/snapshot/create", Some(snap_body.to_string())).await?;
    if status != 204 { bail!("CreateSnapshot failed: {} {}", status, body); }
    println!("  Snapshot saved to {}", snapshot_dir);

    kill_firecracker().await?;
    println!("  Firecracker process killed");
    Ok(())
}

/// Standalone wake: uses fctools VmConfiguration::RestoredFromSnapshot
async fn standalone_snapshot_wake(cfg: &ProfileConfig) -> Result<Duration> {
    let snap_state = format!("{}/vm.snap", cfg.snapshot_dir);
    let snap_mem = format!("{}/vm.mem", cfg.snapshot_dir);
    let start = Instant::now();

    let _ = tokio::fs::remove_file(SOCKET_PATH).await;

    let mut resource_system = fc_resource_system();
    let data = build_vm_config(cfg, &mut resource_system)?;

    // Create resources for snapshot files
    let snap_resource = resource_system
        .create_resource(abs_path(&snap_state), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
        .map_err(|e| anyhow::anyhow!("Failed to create snap resource: {:?}", e))?;
    let mem_resource = resource_system
        .create_resource(abs_path(&snap_mem), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
        .map_err(|e| anyhow::anyhow!("Failed to create mem resource: {:?}", e))?;

    let load_snapshot = LoadSnapshot {
        track_dirty_pages: Some(true),
        mem_backend: MemoryBackend {
            backend_type: MemoryBackendType::File,
            backend: mem_resource,
        },
        snapshot: snap_resource,
        resume_vm: Some(true),
        network_overrides: vec![],
    };

    let configuration = VmConfiguration::RestoredFromSnapshot {
        load_snapshot,
        data,
    };

    let mut vm = Vm::prepare(fc_executor(), resource_system, fc_installation(), configuration)
        .await
        .map_err(|e| anyhow::anyhow!("Vm::prepare (restore) failed: {:?}", e))?;

    vm.start(Duration::from_secs(10))
        .await
        .map_err(|e| anyhow::anyhow!("Vm::start (restore) failed: {:?}", e))?;

    let restore_time = start.elapsed();

    // Save PID for later commands
    // Note: fctools manages the process internally, but we save state for destroy
    save_state(&VmState {
        pid: 0, // fctools manages PID
        socket_path: SOCKET_PATH.into(),
        snapshot_dir: cfg.snapshot_dir.into(),
        snapshot_exists: true,
    }).await?;

    Ok(restore_time)
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

/// Create + start VM using fctools
async fn cmd_create_start(cfg: &ProfileConfig) -> Result<()> {
    println!("Creating and starting VM via fctools...");
    let start = Instant::now();

    let _vm = create_and_start_vm(cfg).await?;

    println!("  Waiting for health check on port {}{}...", cfg.health_port, cfg.health_path);
    if wait_for_health(cfg.boot_timeout, cfg.health_port, cfg.health_path).await? {
        println!("VM started and healthy ({:.1}s total, fctools)", start.elapsed().as_secs_f64());
    } else {
        bail!("VM started but health check failed after {}s", cfg.boot_timeout);
    }

    // Save state — VM process is detached (runs after Vm struct is dropped)
    save_state(&VmState {
        pid: 0,
        socket_path: SOCKET_PATH.into(),
        snapshot_dir: cfg.snapshot_dir.into(),
        snapshot_exists: false,
    }).await?;

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

/// Wake: uses fctools VmConfiguration::RestoredFromSnapshot
async fn cmd_wake(cfg: &ProfileConfig) -> Result<()> {
    println!("Waking VM (snapshot restore via fctools)...");
    let restore_time = standalone_snapshot_wake(cfg).await?;
    println!("  Snapshot restored in {:.0}ms (fctools)", restore_time.as_millis());

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

    // Step 3: Snapshot sleep (pause → snapshot)
    println!("\n--- Step 3: Snapshot sleep ---");
    let snapshot = snapshot_sleep_fctools(&mut vm, cfg).await?;

    // Step 4: Snapshot wake (restore via prepare_vm, which reuses resources from old vm)
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

        // Shutdown old VM, then restore
        // prepare_vm takes &mut old_vm to move resources from old to new VM
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
