use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::TokioIo;
use http_body_util::{BodyExt, Full};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::net::UnixStream;
use tokio::process::Command;

const VM_IP: &str = "172.16.0.2";
const KERNEL_PATH: &str = "resources/vmlinux-6.1";
const SOCKET_PATH: &str = "/tmp/fc-poc.socket";
const LOG_PATH: &str = "/tmp/fc-poc.log";
const PID_FILE: &str = "/tmp/fc-poc.pid";
const TAP_DEV: &str = "tap0";
const GUEST_MAC: &str = "AA:FC:00:00:00:01";

// Stage 1 (basic) defaults
const BASIC_ROOTFS: &str = "resources/rootfs.ext4";
const BASIC_PORT: u16 = 8080;
const BASIC_MEM_MIB: u32 = 256;
const BASIC_HEALTH_PATH: &str = "/health";
const BASIC_BOOT_TIMEOUT: u64 = 30;

// Stage 2 (openclaw) defaults
const OPENCLAW_ROOTFS: &str = "resources/rootfs-openclaw.ext4";
const _OPENCLAW_GATEWAY_PORT: u16 = 3000;
const OPENCLAW_BRIDGE_PORT: u16 = 3001;
const OPENCLAW_MEM_MIB: u32 = 1536;
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
    mem_mib: u32,
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

#[derive(Parser)]
#[command(name = "firecracker-poc", about = "Firecracker MicroVM PoC CLI")]
struct Cli {
    /// VM profile (basic = Stage 1, openclaw = Stage 2)
    #[arg(long, value_enum, default_value_t = Profile::Basic)]
    profile: Profile,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create and configure a new VM (does not start it)
    Create,
    /// Start the VM
    Start,
    /// Health check — verify HTTP server responds
    Check,
    /// Snapshot the VM to disk and kill the Firecracker process
    Stop,
    /// Restore VM from snapshot
    Wake,
    /// Kill the Firecracker process without snapshotting
    Destroy,
    /// Run 5 consecutive stop/wake cycles as a stress test
    Stress,
    /// Send a chat message via the bridge (openclaw profile only)
    Chat {
        /// The message to send
        #[arg(default_value = "Hello! Please respond with a short greeting.")]
        message: String,
    },
}

// Firecracker API request/response types
#[derive(Serialize)]
struct BootSource {
    kernel_image_path: String,
    boot_args: String,
}

#[derive(Serialize)]
struct Drive {
    drive_id: String,
    path_on_host: String,
    is_root_device: bool,
    is_read_only: bool,
}

#[derive(Serialize)]
struct MachineConfig {
    vcpu_count: u32,
    mem_size_mib: u32,
    track_dirty_pages: bool,
}

#[derive(Serialize)]
struct NetworkInterface {
    iface_id: String,
    guest_mac: String,
    host_dev_name: String,
}

#[derive(Serialize)]
struct Action {
    action_type: String,
}

#[derive(Serialize)]
struct SnapshotCreate {
    snapshot_type: String,
    snapshot_path: String,
    mem_file_path: String,
}

#[derive(Serialize)]
struct SnapshotLoad {
    snapshot_path: String,
    mem_backend: MemBackend,
    enable_diff_snapshots: bool,
    resume_vm: bool,
}

#[derive(Serialize)]
struct MemBackend {
    backend_type: String,
    backend_path: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct VmInfo {
    state: String,
}

/// Send an HTTP request to the Firecracker API via Unix socket
async fn fc_request(
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<(u16, String)> {
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
    let body = resp
        .into_body()
        .collect()
        .await
        .context("Failed to read response body")?
        .to_bytes();
    let body_str = String::from_utf8_lossy(&body).to_string();

    Ok((status, body_str))
}

/// Start a new Firecracker process
async fn start_firecracker_process() -> Result<u32> {
    // Clean up old socket
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;

    // Create log file
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

    // Wait for socket to appear
    for _ in 0..50 {
        if Path::new(SOCKET_PATH).exists() {
            // Make socket accessible to non-root (FC runs as root via sudo)
            let _ = Command::new("sudo")
                .arg("chmod")
                .arg("666")
                .arg(SOCKET_PATH)
                .output()
                .await;
            // Save PID
            tokio::fs::write(PID_FILE, pid.to_string()).await?;
            return Ok(pid);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    bail!("Firecracker socket did not appear after 5s");
}

/// Kill the Firecracker process
async fn kill_firecracker() -> Result<()> {
    if let Ok(pid_str) = tokio::fs::read_to_string(PID_FILE).await {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            // The PID file stores the sudo process PID; find the actual FC child
            let _ = Command::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg(format!("kill {} 2>/dev/null; sleep 1; kill -9 {} 2>/dev/null", pid, pid))
                .output()
                .await;
            // Also kill any firecracker process that might be a child of sudo
            let _ = Command::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg("pkill -f 'firecracker --api-sock /tmp/fc-poc' 2>/dev/null")
                .output()
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    let _ = tokio::fs::remove_file(PID_FILE).await;
    let _ = tokio::fs::remove_file(SOCKET_PATH).await;
    Ok(())
}

/// Get absolute path relative to the project directory
fn abs_path(relative: &str) -> String {
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    base.join(relative).to_string_lossy().to_string()
}

/// Configure the VM (boot source, drives, machine config, network)
async fn configure_vm(cfg: &ProfileConfig) -> Result<()> {
    // Boot source
    let boot = BootSource {
        kernel_image_path: abs_path(KERNEL_PATH),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/fc-init".into(),
    };
    let (status, body) = fc_request("PUT", "/boot-source", Some(serde_json::to_string(&boot)?)).await?;
    if status != 204 {
        bail!("boot-source failed: {} {}", status, body);
    }

    // Root drive
    let drive = Drive {
        drive_id: "rootfs".into(),
        path_on_host: abs_path(cfg.rootfs),
        is_root_device: true,
        is_read_only: false,
    };
    let (status, body) = fc_request("PUT", "/drives/rootfs", Some(serde_json::to_string(&drive)?)).await?;
    if status != 204 {
        bail!("drives failed: {} {}", status, body);
    }

    // Machine config — OpenClaw needs more memory for Node.js
    let config = MachineConfig {
        vcpu_count: 2,
        mem_size_mib: cfg.mem_mib,
        track_dirty_pages: true,
    };
    let (status, body) = fc_request("PUT", "/machine-config", Some(serde_json::to_string(&config)?)).await?;
    if status != 204 {
        bail!("machine-config failed: {} {}", status, body);
    }

    // Network interface
    let net = NetworkInterface {
        iface_id: "eth0".into(),
        guest_mac: GUEST_MAC.into(),
        host_dev_name: TAP_DEV.into(),
    };
    let (status, body) = fc_request(
        "PUT",
        "/network-interfaces/eth0",
        Some(serde_json::to_string(&net)?),
    ).await?;
    if status != 204 {
        bail!("network-interfaces failed: {} {}", status, body);
    }

    Ok(())
}

/// Start the VM (InstanceStart action)
async fn instance_start() -> Result<()> {
    let action = Action {
        action_type: "InstanceStart".into(),
    };
    let (status, body) = fc_request("PUT", "/actions", Some(serde_json::to_string(&action)?)).await?;
    if status != 204 {
        bail!("InstanceStart failed: {} {}", status, body);
    }
    Ok(())
}

/// Check if the HTTP server in the VM responds
async fn health_check(port: u16, path: &str) -> Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("{}:{}", VM_IP, port);
    let stream = match tokio::time::timeout(
        std::time::Duration::from_secs(3),
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
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        reader.read(&mut buf),
    ).await {
        Ok(Ok(n)) if n > 0 => {
            let resp = String::from_utf8_lossy(&buf[..n]);
            Ok(resp.contains("200") || resp.contains("ok"))
        }
        _ => Ok(false),
    }
}

/// Wait for the VM to become healthy
async fn wait_for_health(timeout_secs: u64, port: u16, path: &str) -> Result<bool> {
    let deadline = Instant::now() + std::time::Duration::from_secs(timeout_secs);
    while Instant::now() < deadline {
        if health_check(port, path).await.unwrap_or(false) {
            return Ok(true);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Ok(false)
}

/// Create snapshot (pause → snapshot → kill)
async fn snapshot_sleep(snapshot_dir: &str) -> Result<()> {
    let snap_state = format!("{}/vm.snap", snapshot_dir);
    let snap_mem = format!("{}/vm.mem", snapshot_dir);

    // Ensure snapshot directory exists
    tokio::fs::create_dir_all(snapshot_dir).await?;

    // Pause the VM (PATCH /vm with state: Paused)
    let (status, body) = fc_request(
        "PATCH",
        "/vm",
        Some(r#"{"state": "Paused"}"#.to_string()),
    ).await?;
    if status != 204 {
        bail!("Pause failed: {} {}", status, body);
    }
    println!("  VM paused");

    // Create snapshot
    let snap = SnapshotCreate {
        snapshot_type: "Full".into(),
        snapshot_path: abs_path(&snap_state),
        mem_file_path: abs_path(&snap_mem),
    };
    let (status, body) = fc_request("PUT", "/snapshot/create", Some(serde_json::to_string(&snap)?)).await?;
    if status != 204 {
        bail!("CreateSnapshot failed: {} {}", status, body);
    }
    println!("  Snapshot saved to {}", snapshot_dir);

    // Kill the Firecracker process
    kill_firecracker().await?;
    println!("  Firecracker process killed");

    Ok(())
}

/// Restore from snapshot
async fn snapshot_wake(snapshot_dir: &str) -> Result<std::time::Duration> {
    let snap_state = format!("{}/vm.snap", snapshot_dir);
    let snap_mem = format!("{}/vm.mem", snapshot_dir);
    let start = Instant::now();

    // Start a new Firecracker process
    let pid = start_firecracker_process().await?;
    println!("  New Firecracker process: PID {}", pid);

    // Load snapshot
    let load = SnapshotLoad {
        snapshot_path: abs_path(&snap_state),
        mem_backend: MemBackend {
            backend_type: "File".into(),
            backend_path: abs_path(&snap_mem),
        },
        enable_diff_snapshots: false,
        resume_vm: true,
    };
    let (status, body) = fc_request("PUT", "/snapshot/load", Some(serde_json::to_string(&load)?)).await?;
    if status != 204 {
        bail!("LoadSnapshot failed: {} {}", status, body);
    }

    let restore_time = start.elapsed();
    Ok(restore_time)
}

// Commands

async fn cmd_create(cfg: &ProfileConfig) -> Result<()> {
    println!("Creating VM...");

    let pid = start_firecracker_process().await?;
    println!("  Firecracker process started: PID {}", pid);

    configure_vm(cfg).await?;
    println!("  VM configured (kernel={}, rootfs={}, mem={}MB)", KERNEL_PATH, cfg.rootfs, cfg.mem_mib);
    println!("VM created successfully. Run 'start' to boot it.");
    Ok(())
}

async fn cmd_start(cfg: &ProfileConfig) -> Result<()> {
    println!("Starting VM (timeout={}s)...", cfg.boot_timeout);
    let start = Instant::now();

    instance_start().await?;
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

async fn cmd_stop(cfg: &ProfileConfig) -> Result<()> {
    println!("Stopping VM (snapshot sleep)...");
    snapshot_sleep(cfg.snapshot_dir).await?;
    println!("VM stopped. Snapshot saved.");
    Ok(())
}

async fn cmd_wake(cfg: &ProfileConfig) -> Result<()> {
    println!("Waking VM (snapshot restore)...");
    let restore_time = snapshot_wake(cfg.snapshot_dir).await?;
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
    println!("VM destroyed.");
    Ok(())
}

async fn cmd_stress(cfg: &ProfileConfig) -> Result<()> {
    println!("=== 5-cycle stress test ===\n");
    let mut restore_times = Vec::new();

    for i in 1..=5 {
        println!("--- Cycle {}/5 ---", i);

        // Stop (snapshot)
        println!("  Stopping (snapshot)...");
        snapshot_sleep(cfg.snapshot_dir).await?;
        println!("  Stopped.");

        // Wake (restore)
        println!("  Waking (restore)...");
        let restore_time = snapshot_wake(cfg.snapshot_dir).await?;
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
        std::time::Duration::from_secs(5),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .context("Connection timeout")?
    .context("Failed to connect to bridge")?;

    let (mut reader, mut writer) = stream.into_split();

    let req = format!(
        "POST /chat HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        addr,
        body.len(),
        body
    );
    writer.write_all(req.as_bytes()).await?;

    println!("\n--- Response (SSE stream) ---");
    let mut buf = vec![0u8; 8192];
    let mut accumulated = String::new();
    let deadline = Instant::now() + std::time::Duration::from_secs(120);

    loop {
        if Instant::now() > deadline {
            println!("\n[Timeout after 120s]");
            break;
        }

        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            reader.read(&mut buf),
        ).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                let chunk = String::from_utf8_lossy(&buf[..n]);
                accumulated.push_str(&chunk);

                // Parse SSE events from accumulated data
                while let Some(data_start) = accumulated.find("data: ") {
                    let rest = &accumulated[data_start + 6..];
                    if let Some(end) = rest.find('\n') {
                        let data_line = &rest[..end].trim();
                        if *data_line == "[DONE]" {
                            println!("\n--- Chat complete ---");
                            return Ok(());
                        }
                        // Try to parse as JSON
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
                        break; // Incomplete line, wait for more data
                    }
                }
            }
            Ok(Err(e)) => {
                bail!("Read error: {}", e);
            }
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

    println!("[profile: {:?}]", cli.profile);

    match cli.command {
        Commands::Create => cmd_create(&cfg).await,
        Commands::Start => cmd_start(&cfg).await,
        Commands::Check => cmd_check(&cfg).await,
        Commands::Stop => cmd_stop(&cfg).await,
        Commands::Wake => cmd_wake(&cfg).await,
        Commands::Destroy => cmd_destroy(&cfg).await,
        Commands::Stress => cmd_stress(&cfg).await,
        Commands::Chat { message } => cmd_chat(&message).await,
    }
}
