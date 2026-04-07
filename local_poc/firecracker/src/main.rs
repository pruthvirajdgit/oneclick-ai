use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
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
const VM_PORT: u16 = 8080;
const KERNEL_PATH: &str = "resources/vmlinux-6.1";
const ROOTFS_PATH: &str = "resources/rootfs.ext4";
const SOCKET_PATH: &str = "/tmp/fc-poc.socket";
const LOG_PATH: &str = "/tmp/fc-poc.log";
const PID_FILE: &str = "/tmp/fc-poc.pid";
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_STATE: &str = "snapshots/vm.snap";
const SNAPSHOT_MEM: &str = "snapshots/vm.mem";
const TAP_DEV: &str = "tap0";
const GUEST_MAC: &str = "AA:FC:00:00:00:01";

#[derive(Parser)]
#[command(name = "firecracker-poc", about = "Firecracker MicroVM PoC CLI")]
struct Cli {
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

    let child = Command::new("firecracker")
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
            let _ = Command::new("kill")
                .arg(pid.to_string())
                .output()
                .await;
            // Wait a moment for clean shutdown
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // Force kill if still alive
            let _ = Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .output()
                .await;
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
async fn configure_vm() -> Result<()> {
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
        path_on_host: abs_path(ROOTFS_PATH),
        is_root_device: true,
        is_read_only: false,
    };
    let (status, body) = fc_request("PUT", "/drives/rootfs", Some(serde_json::to_string(&drive)?)).await?;
    if status != 204 {
        bail!("drives failed: {} {}", status, body);
    }

    // Machine config
    let config = MachineConfig {
        vcpu_count: 2,
        mem_size_mib: 256,
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
async fn health_check() -> Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("{}:{}", VM_IP, VM_PORT);
    let stream = match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    ).await {
        Ok(Ok(s)) => s,
        _ => return Ok(false),
    };

    let (mut reader, mut writer) = stream.into_split();
    let req = format!("GET /health HTTP/1.0\r\nHost: {}\r\n\r\n", addr);
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
async fn wait_for_health(timeout_secs: u64) -> Result<bool> {
    let deadline = Instant::now() + std::time::Duration::from_secs(timeout_secs);
    while Instant::now() < deadline {
        if health_check().await.unwrap_or(false) {
            return Ok(true);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Ok(false)
}

/// Create snapshot (pause → snapshot → kill)
async fn snapshot_sleep() -> Result<()> {
    // Ensure snapshot directory exists
    tokio::fs::create_dir_all(SNAPSHOT_DIR).await?;

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
        snapshot_path: abs_path(SNAPSHOT_STATE),
        mem_file_path: abs_path(SNAPSHOT_MEM),
    };
    let (status, body) = fc_request("PUT", "/snapshot/create", Some(serde_json::to_string(&snap)?)).await?;
    if status != 204 {
        bail!("CreateSnapshot failed: {} {}", status, body);
    }
    println!("  Snapshot saved to {}", SNAPSHOT_DIR);

    // Kill the Firecracker process
    kill_firecracker().await?;
    println!("  Firecracker process killed");

    Ok(())
}

/// Restore from snapshot
async fn snapshot_wake() -> Result<std::time::Duration> {
    let start = Instant::now();

    // Start a new Firecracker process
    let pid = start_firecracker_process().await?;
    println!("  New Firecracker process: PID {}", pid);

    // Load snapshot
    let load = SnapshotLoad {
        snapshot_path: abs_path(SNAPSHOT_STATE),
        mem_backend: MemBackend {
            backend_type: "File".into(),
            backend_path: abs_path(SNAPSHOT_MEM),
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

async fn cmd_create() -> Result<()> {
    println!("Creating VM...");

    let pid = start_firecracker_process().await?;
    println!("  Firecracker process started: PID {}", pid);

    configure_vm().await?;
    println!("  VM configured (kernel, rootfs, network)");
    println!("VM created successfully. Run 'start' to boot it.");
    Ok(())
}

async fn cmd_start() -> Result<()> {
    println!("Starting VM...");
    let start = Instant::now();

    instance_start().await?;
    println!("  InstanceStart sent ({:.0}ms)", start.elapsed().as_millis());

    println!("  Waiting for health check...");
    if wait_for_health(30).await? {
        println!("VM started and healthy ({:.0}ms total)", start.elapsed().as_millis());
    } else {
        bail!("VM started but health check failed after 30s");
    }
    Ok(())
}

async fn cmd_check() -> Result<()> {
    print!("Health check... ");
    let start = Instant::now();
    if health_check().await? {
        println!("OK ({:.0}ms)", start.elapsed().as_millis());
    } else {
        println!("FAILED");
        bail!("Health check failed — VM HTTP server not responding");
    }
    Ok(())
}

async fn cmd_stop() -> Result<()> {
    println!("Stopping VM (snapshot sleep)...");
    snapshot_sleep().await?;
    println!("VM stopped. Snapshot saved.");
    Ok(())
}

async fn cmd_wake() -> Result<()> {
    println!("Waking VM (snapshot restore)...");
    let restore_time = snapshot_wake().await?;
    println!("  Snapshot restored in {:.0}ms", restore_time.as_millis());

    println!("  Waiting for health check...");
    let start = Instant::now();
    if wait_for_health(10).await? {
        println!("VM awake and healthy ({:.0}ms after restore)", start.elapsed().as_millis());
    } else {
        bail!("VM restored but health check failed after 10s");
    }
    Ok(())
}

async fn cmd_destroy() -> Result<()> {
    println!("Destroying VM...");
    kill_firecracker().await?;
    // Clean up snapshots
    let _ = tokio::fs::remove_file(SNAPSHOT_STATE).await;
    let _ = tokio::fs::remove_file(SNAPSHOT_MEM).await;
    println!("VM destroyed.");
    Ok(())
}

async fn cmd_stress() -> Result<()> {
    println!("=== 5-cycle stress test ===\n");
    let mut restore_times = Vec::new();

    for i in 1..=5 {
        println!("--- Cycle {}/5 ---", i);

        // Stop (snapshot)
        println!("  Stopping (snapshot)...");
        snapshot_sleep().await?;
        println!("  Stopped.");

        // Wake (restore)
        println!("  Waking (restore)...");
        let restore_time = snapshot_wake().await?;
        let ms = restore_time.as_millis();
        restore_times.push(ms);
        println!("  Restored in {}ms", ms);

        // Health check
        print!("  Health check... ");
        let start = Instant::now();
        if wait_for_health(10).await? {
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Create => cmd_create().await,
        Commands::Start => cmd_start().await,
        Commands::Check => cmd_check().await,
        Commands::Stop => cmd_stop().await,
        Commands::Wake => cmd_wake().await,
        Commands::Destroy => cmd_destroy().await,
        Commands::Stress => cmd_stress().await,
    }
}
