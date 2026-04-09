//! TAP network device pool manager for Firecracker VMs.
//!
//! Each VM gets a dedicated TAP device and a `/30` subnet:
//!   - Host side: `{prefix}.0.{4i+1}`  (gateway)
//!   - Guest side: `{prefix}.0.{4i+2}` (VM IP)
//!
//! The pool is fixed-size (`tap0..tap{count-1}`) and devices are allocated
//! on demand, returned when VMs are destroyed.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::info;

use oneclick_shared::config::Config;

/// A single TAP allocation with associated network addresses.
#[derive(Debug, Clone)]
pub struct TapAllocation {
    /// TAP device name (e.g. "tap0")
    pub device: String,
    /// Host-side IP (e.g. "172.16.0.1")
    pub host_ip: String,
    /// Guest-side IP (e.g. "172.16.0.2")
    pub guest_ip: String,
    /// CIDR notation for the /30 subnet (e.g. "172.16.0.0/30")
    pub cidr: String,
    /// Guest MAC address
    pub guest_mac: String,
    /// Pool index
    pub index: usize,
}

/// Manages a pool of TAP devices for Firecracker VMs.
pub struct TapManager {
    prefix: String,
    subnet_prefix: String,
    count: usize,
    /// Maps pool index → agent_id (None = available)
    inner: Arc<Mutex<TapManagerInner>>,
}

struct TapManagerInner {
    /// pool index → Option<agent_id>
    slots: Vec<Option<String>>,
    /// agent_id → TapAllocation
    allocations: HashMap<String, TapAllocation>,
}

impl TapManager {
    /// Create a new TAP manager from config.
    pub fn new(config: &Config) -> Self {
        let count = config.fc_tap_count as usize;
        // Validate: each TAP uses 4 IPs in the last two octets.
        // With subnet_prefix like "172.16", addresses are {prefix}.{third}.{fourth}.
        // Max safe count: (256 * 256) / 4 = 16384, but practically capped at 64
        // to stay within a single /24 (third octet = 0).
        assert!(
            count <= 64,
            "FC_TAP_COUNT must be <= 64 to fit in a /24 subnet (got {count})"
        );
        Self {
            prefix: config.fc_tap_prefix.clone(),
            subnet_prefix: config.fc_subnet_prefix.clone(),
            count,
            inner: Arc::new(Mutex::new(TapManagerInner {
                slots: vec![None; count],
                allocations: HashMap::new(),
            })),
        }
    }

    /// Compute addresses for a given pool index.
    fn addresses(&self, index: usize) -> (String, String, String, String) {
        let base = index * 4;
        let host_ip = format!("{}.0.{}", self.subnet_prefix, base + 1);
        let guest_ip = format!("{}.0.{}", self.subnet_prefix, base + 2);
        let cidr = format!("{}.0.{}/30", self.subnet_prefix, base);
        let device = format!("{}{}", self.prefix, index);
        (device, host_ip, guest_ip, cidr)
    }

    /// Generate a deterministic MAC from pool index.
    fn guest_mac(index: usize) -> String {
        format!("AA:FC:00:00:00:{:02X}", index)
    }

    /// Allocate a TAP device for an agent. Sets up the device if needed.
    pub async fn allocate(&self, agent_id: &str) -> Result<TapAllocation, String> {
        let mut inner = self.inner.lock().await;

        // Check if already allocated
        if let Some(alloc) = inner.allocations.get(agent_id) {
            return Ok(alloc.clone());
        }

        // Find a free slot
        let index = inner
            .slots
            .iter()
            .position(|s| s.is_none())
            .ok_or_else(|| format!("No free TAP devices (pool size: {})", self.count))?;

        let (device, host_ip, guest_ip, cidr) = self.addresses(index);
        let mac = Self::guest_mac(index);

        // Setup the TAP device on the host
        self.setup_tap_device(&device, &host_ip).await?;

        let alloc = TapAllocation {
            device: device.clone(),
            host_ip,
            guest_ip,
            cidr,
            guest_mac: mac,
            index,
        };

        inner.slots[index] = Some(agent_id.to_string());
        inner.allocations.insert(agent_id.to_string(), alloc.clone());

        info!(
            agent_id,
            tap_device = %device,
            guest_ip = %alloc.guest_ip,
            "TAP device allocated"
        );

        Ok(alloc)
    }

    /// Release a TAP device back to the pool.
    pub async fn release(&self, agent_id: &str) {
        let mut inner = self.inner.lock().await;
        if let Some(alloc) = inner.allocations.remove(agent_id) {
            if alloc.index < inner.slots.len() {
                inner.slots[alloc.index] = None;
            }
            info!(
                agent_id,
                tap_device = %alloc.device,
                "TAP device released"
            );
        }
    }

    /// Get the allocation for an agent (if any).
    pub async fn get_allocation(&self, agent_id: &str) -> Option<TapAllocation> {
        let inner = self.inner.lock().await;
        inner.allocations.get(agent_id).cloned()
    }

    /// Set up a TAP device with the given host IP.
    async fn setup_tap_device(&self, device: &str, host_ip: &str) -> Result<(), String> {
        // Create TAP device (ignore error if already exists)
        let _ = run_cmd("sudo", &["ip", "tuntap", "add", "dev", device, "mode", "tap"]).await;

        // Set the host IP on the device
        // First flush any existing addresses
        let _ = run_cmd("sudo", &["ip", "addr", "flush", "dev", device]).await;

        run_cmd(
            "sudo",
            &["ip", "addr", "add", &format!("{}/30", host_ip), "dev", device],
        )
        .await
        .map_err(|e| format!("Failed to set IP on {device}: {e}"))?;

        // Bring the device up
        run_cmd("sudo", &["ip", "link", "set", device, "up"])
            .await
            .map_err(|e| format!("Failed to bring up {device}: {e}"))?;

        // Enable IP forwarding
        let _ = run_cmd("sudo", &["sysctl", "-w", "net.ipv4.ip_forward=1"]).await;

        // Detect default egress interface
        let route_output = run_cmd("ip", &["route", "show", "default"])
            .await
            .unwrap_or_default();
        let default_if = route_output
            .split_whitespace()
            .skip_while(|&w| w != "dev")
            .nth(1)
            .unwrap_or("eth0");

        // Only add MASQUERADE rule if it doesn't already exist
        let check_result = run_cmd(
            "sudo",
            &[
                "iptables", "-t", "nat", "-C", "POSTROUTING",
                "-o", default_if, "-j", "MASQUERADE",
            ],
        )
        .await;

        if check_result.is_err() {
            let _ = run_cmd(
                "sudo",
                &[
                    "iptables", "-t", "nat", "-A", "POSTROUTING",
                    "-o", default_if, "-j", "MASQUERADE",
                ],
            )
            .await;
        }

        info!(device, host_ip, "TAP device configured");
        Ok(())
    }
}

/// Run a shell command, returning stdout on success or an error message.
async fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to run {program}: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{} {} failed: {}",
            program,
            args.join(" "),
            stderr.trim()
        ))
    }
}
