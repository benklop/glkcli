#[cfg(feature = "network-check")]
use anyhow::Result;

/// Network connectivity checker that works with IWD or NetworkManager via D-Bus
pub struct NetworkChecker {
    debug: bool,
    #[allow(dead_code)]
    assume_online: bool,
}

impl NetworkChecker {
    pub fn new(debug: bool, assume_online: bool) -> Self {
        Self { debug, assume_online }
    }

    /// Check if the system has network connectivity
    /// Tries IWD first, then NetworkManager via D-Bus (if network-check feature enabled)
    pub async fn is_connected(&self) -> bool {
        // If assume_online flag is set, always return true
        if self.assume_online {
            if self.debug {
                log::debug!("Network check disabled - assuming online");
            }
            return true;
        }

        #[cfg(feature = "network-check")]
        {
            self.check_with_dbus().await
        }

        #[cfg(not(feature = "network-check"))]
        {
            if self.debug {
                log::debug!("Network check disabled at compile time - assuming online");
            }
            true
        }
    }

    #[cfg(feature = "network-check")]
    async fn check_with_dbus(&self) -> bool {
        // Try IWD first (common on embedded/minimal systems)
        match self.check_iwd().await {
            Ok(connected) => {
                if self.debug {
                    log::debug!("IWD connectivity check: {}", connected);
                }
                return connected;
            }
            Err(e) => {
                if self.debug {
                    log::debug!("IWD check failed: {}", e);
                }
            }
        }

        // Try NetworkManager (most common on desktop Linux)
        match self.check_networkmanager().await {
            Ok(connected) => {
                if self.debug {
                    log::debug!("NetworkManager connectivity check: {}", connected);
                }
                return connected;
            }
            Err(e) => {
                if self.debug {
                    log::debug!("NetworkManager check failed: {}", e);
                }
            }
        }

        // No network managers available or both report offline
        if self.debug {
            log::warn!("No network managers available or all report offline");
        }
        false
    }

    #[cfg(feature = "network-check")]
    /// Check connectivity via IWD D-Bus interface
    async fn check_iwd(&self) -> Result<bool> {
        use std::process::Command;
        
        // Use busctl to query IWD - simple and doesn't require extra dependencies
        // Set a short timeout to fail fast if IWD isn't available
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::task::spawn_blocking(|| {
                Command::new("busctl")
                    .args(&[
                        "call",
                        "--system",
                        "net.connman.iwd",
                        "/net/connman/iwd",
                        "org.freedesktop.DBus.ObjectManager",
                        "GetManagedObjects"
                    ])
                    .output()
            })
        ).await???;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Look for "State" property with "connected" value
            Ok(stdout.contains("State") && stdout.contains("connected"))
        } else {
            Err(anyhow::anyhow!("IWD not available"))
        }
    }

    #[cfg(feature = "network-check")]
    /// Check connectivity via NetworkManager D-Bus interface
    async fn check_networkmanager(&self) -> Result<bool> {
        use std::process::Command;
        
        // Use busctl to query NetworkManager with a short timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::task::spawn_blocking(|| {
                Command::new("busctl")
                    .args(&[
                        "get-property",
                        "--system",
                        "org.freedesktop.NetworkManager",
                        "/org/freedesktop/NetworkManager",
                        "org.freedesktop.NetworkManager",
                        "State"
                    ])
                    .output()
            })
        ).await???;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // NetworkManager State: 70 = NM_STATE_CONNECTED_GLOBAL
            // Output format is: "u 70" or similar
            if let Some(state_str) = stdout.split_whitespace().nth(1) {
                if let Ok(state) = state_str.parse::<u32>() {
                    return Ok(state >= 70);
                }
            }
        }
        
        Err(anyhow::anyhow!("NetworkManager not available"))
    }
}
