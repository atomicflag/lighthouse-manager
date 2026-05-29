use anyhow::{Context, Result, anyhow, bail};
pub use btleplug::platform::Adapter;
pub use btleplug::platform::Manager;
pub use btleplug::platform::Peripheral;

use btleplug::api::{BDAddr, Central as _, Manager as _, Peripheral as _, ScanFilter, WriteType};
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::lighthouse::Lighthouse;
use crate::protocol;

/// Discover nearby lighthouses by scanning BLE advertisements for a given duration.
/// Filters results to only devices whose name starts with "HTC BS" or "LHB-".
///
/// # Errors
///
/// Returns an error if the BLE scan fails to start, stop, or retrieve peripherals,
/// or if any underlying Bluetooth adapter operation fails.
pub async fn discover_lighthouses(adapter: &Adapter, timeout_secs: u64) -> Result<Vec<Lighthouse>> {
    info!(
        "Starting Bluetooth LE discovery for {} seconds...",
        timeout_secs
    );

    adapter
        .start_scan(ScanFilter {
            services: vec![], // Scan all to catch name-based matches
        })
        .await
        .context("Failed to start BLE scan")?;

    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

    adapter
        .stop_scan()
        .await
        .context("Failed to stop BLE scan")?;

    let peripherals = adapter
        .peripherals()
        .await
        .context("Failed to get discovered peripherals")?;

    info!("Scan complete. Found {} raw devices", peripherals.len());

    // Filter for lighthouse name patterns
    let mut lighthouses = Vec::new();
    for peripheral in &peripherals {
        let address_str = peripheral.address().to_string();
        if let Some(name) = get_local_name(peripheral).await
            && is_lighthouse_name(&name)
        {
            let lh = Lighthouse {
                name,
                address: address_str.clone(),
                id: None, // Will be filled from DB if available
                managed: false,
            };
            debug!("Discovered lighthouse: {} ({})", lh.name, address_str);
            lighthouses.push(lh);
        }
    }

    info!("Found {} Lighthouse(s)", lighthouses.len());
    Ok(lighthouses)
}

/// Check if a device name matches known Lighthouse naming patterns.
fn is_lighthouse_name(name: &str) -> bool {
    name.starts_with("HTC BS") || name.starts_with("LHB-")
}

/// Get the local name from a peripheral's properties.
async fn get_local_name(peripheral: &Peripheral) -> Option<String> {
    peripheral.properties().await.ok()??.local_name
}

/// Connect to a specific lighthouse by its Bluetooth address.
///
/// # Errors
///
/// Returns an error if the address is invalid, the peripheral is not found,
/// or any BLE connection/discovery operation fails.
pub async fn connect_lighthouse(
    adapter: &Adapter,
    address_str: &str,
) -> Result<ConnectedPeripheral> {
    let target_addr = BDAddr::from_str(address_str)
        .map_err(|e| anyhow!("Invalid Bluetooth address format '{address_str}': {e}"))?;

    // Find the peripheral in the adapter's known peripherals
    let peripherals = adapter
        .peripherals()
        .await
        .context("Failed to get adapter peripherals")?;

    let peripheral = peripherals
        .into_iter()
        .find(|p| p.address() == target_addr)
        .ok_or_else(|| anyhow!("Peripheral not found: {address_str}"))?;

    info!("Connecting to {}...", peripheral.address());
    peripheral
        .connect()
        .await
        .context("Failed to connect to device")?;
    info!("Connected to {}", peripheral.address());

    // Discover services and characteristics
    peripheral
        .discover_services()
        .await
        .context("Failed to discover GATT services")?;
    debug!("Services discovered for {}", peripheral.address());

    Ok(ConnectedPeripheral { peripheral })
}

/// A connected lighthouse device ready for GATT operations.
pub struct ConnectedPeripheral {
    pub(crate) peripheral: Peripheral,
}

impl ConnectedPeripheral {
    /// Write data to a characteristic and disconnect.
    async fn write_and_disconnect(&self, uuid_str: &str, data: &[u8]) -> Result<()> {
        let uuid = Uuid::parse_str(uuid_str).map_err(|_| anyhow!("Invalid UUID: {uuid_str}"))?;

        // Retry logic: 5 attempts × 1s delay
        for attempt in 1..=5 {
            debug!(
                "Write attempt {}/5 to characteristic {} on device {}",
                attempt,
                uuid_str,
                self.peripheral.address()
            );

            match self.write_characteristic(&uuid, data).await {
                Ok(()) => {
                    info!(
                        "Successfully wrote {} bytes to {} on {}",
                        data.len(),
                        uuid_str,
                        self.peripheral.address()
                    );
                    return Ok(());
                }
                Err(e) if attempt < 5 => {
                    warn!("Write attempt {} failed: {}. Retrying in 1s...", attempt, e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    bail!("Failed to write to characteristic {uuid_str} after 5 attempts: {e}");
                }
            }
        }
        unreachable!()
    }

    async fn write_characteristic(&self, uuid: &Uuid, data: &[u8]) -> Result<()> {
        let chars = self.peripheral.characteristics();
        let char = chars.iter().find(|c| c.uuid == *uuid).ok_or_else(|| {
            anyhow!(
                "Characteristic {} not found on device {} (has {} characteristics)",
                uuid,
                self.peripheral.address(),
                chars.len()
            )
        })?;

        self.peripheral
            .write(char, data, WriteType::WithoutResponse)
            .await
            .context("Failed to write characteristic")?;
        Ok(())
    }

    /// Power on the connected lighthouse.
    ///
    /// # Errors
    ///
    /// Returns an error if the power command cannot be built (e.g. missing V1 ID)
    /// or if writing to the GATT characteristic fails after retries.
    pub async fn power_on(&self, lh: &Lighthouse) -> Result<()> {
        let cmd = protocol::build_power_command(lh).map_err(|e| anyhow!("{e}"))?;
        self.write_and_disconnect(lh.power_characteristic(), &cmd)
            .await
    }

    /// Sleep the connected lighthouse.
    ///
    /// # Errors
    ///
    /// Returns an error if the sleep command cannot be built (e.g. missing V1 ID)
    /// or if writing to the GATT characteristic fails after retries.
    pub async fn sleep(&self, lh: &Lighthouse) -> Result<()> {
        let cmd = protocol::build_sleep_command(lh).map_err(|e| anyhow!("{e}"))?;
        self.write_and_disconnect(lh.power_characteristic(), &cmd)
            .await
    }

    /// Identify the connected lighthouse (V2 only — causes LED flash).
    ///
    /// # Errors
    ///
    /// Returns an error if the lighthouse is not V2, or if writing to the
    /// identify characteristic fails after retries.
    pub async fn identify(&self, lh: &Lighthouse) -> Result<()> {
        protocol::build_identify_command(lh).map_err(|e| anyhow!("{e}"))?; // validates version
        let cmd = protocol::build_v2_identify();
        let uuid = lh
            .identify_characteristic()
            .ok_or_else(|| anyhow!("Identify is not supported on this lighthouse"))?;
        self.write_and_disconnect(uuid, &cmd).await
    }

    /// Disconnect the device.
    pub async fn disconnect(self) {
        info!("Disconnecting from {}...", self.peripheral.address());
        if let Err(e) = self.peripheral.disconnect().await {
            warn!("Failed to disconnect {}: {}", self.peripheral.address(), e);
        } else {
            debug!("Disconnected from {}", self.peripheral.address());
        }
    }
}

/// Start a BLE scan, poll until a predicate is satisfied (or timeout), then stop the scan.
/// Returns the final set of discovered peripheral addresses (lowercase).
///
/// # Errors
///
/// Returns an error if the initial BLE scan fails to start.
pub async fn scan_until_predicate<F>(adapter: &Adapter, predicate: F) -> Result<HashSet<String>>
where
    F: Fn(&HashSet<String>) -> bool + Send,
{
    adapter
        .start_scan(ScanFilter { services: vec![] })
        .await
        .context("Failed to start BLE scan")?;

    let poll_interval = Duration::from_millis(500);
    let timeout = Duration::from_secs(15);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        if let Ok(peripherals) = adapter.peripherals().await {
            let discovered: HashSet<String> = peripherals
                .iter()
                .map(|p| p.address().to_string().to_lowercase())
                .collect();

            if predicate(&discovered) {
                break;
            }
        }

        tokio::time::sleep(poll_interval).await;
    }

    adapter.stop_scan().await.ok();

    // Return the final discovered set.
    Ok(if let Ok(peripherals) = adapter.peripherals().await {
        peripherals
            .iter()
            .map(|p| p.address().to_string().to_lowercase())
            .collect()
    } else {
        HashSet::new()
    })
}

/// Get the first available Bluetooth adapter. Returns an error if none found.
///
/// # Errors
///
/// Returns an error if the BLE manager cannot be created, no adapters are enumerated,
/// or no adapters are available.
pub async fn get_adapter() -> Result<Adapter> {
    let manager = Manager::new()
        .await
        .map_err(|e| anyhow!("Failed to create BLE manager: {e}"))?;
    let adapters = manager
        .adapters()
        .await
        .context("Failed to enumerate Bluetooth adapters")?;

    if adapters.is_empty() {
        bail!("No Bluetooth adapter found. Please ensure a Bluetooth adapter is available.");
    }

    let adapter = &adapters[0];
    // Use adapter_info for debug output since Adapter doesn't have address() method
    info!(
        "Using Bluetooth adapter at index 0 ({} adapters total)",
        adapters.len()
    );
    Ok(adapter.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_lighthouse_name_v1() {
        assert!(is_lighthouse_name("HTC BS-AABBCCDD"));
        assert!(is_lighthouse_name("HTC BS-12345678"));
        assert!(!is_lighthouse_name("OtherDevice"));
    }

    #[test]
    fn test_is_lighthouse_name_v2() {
        assert!(is_lighthouse_name("LHB-0A1B2C3D"));
        assert!(is_lighthouse_name("LHB-AABBCCDD"));
        assert!(!is_lighthouse_name("LBH-Something"));
    }
}
