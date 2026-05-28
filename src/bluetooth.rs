use anyhow::{Context, Result, anyhow, bail};
pub use btleplug::platform::Adapter;
pub use btleplug::platform::Manager;
pub use btleplug::platform::Peripheral;

use btleplug::api::{BDAddr, Central as _, Manager as _, Peripheral as _, ScanFilter, WriteType};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::lighthouse::Lighthouse;
use crate::protocol::{
    self, V1_POWER_CHARACTERISTIC, V2_IDENTIFY_CHARACTERISTIC, V2_POWER_CHARACTERISTIC,
};

/// Discover nearby lighthouses by scanning BLE advertisements for a given duration.
/// Filters results to only devices whose name starts with "HTC BS" or "LHB-".
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
        let address_str = bdaddr_to_string(&peripheral.address());
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
    let props_result = peripheral.properties().await;
    match props_result {
        Ok(Some(props)) => props.local_name,
        Ok(None) | Err(_) => None,
    }
}

/// Convert BDAddr to colon-separated string like "AA:BB:CC:DD:EE:FF".
fn bdaddr_to_string(addr: &BDAddr) -> String {
    addr.to_string()
}

/// Connect to a specific lighthouse by its Bluetooth address.
pub async fn connect_lighthouse(
    adapter: &Adapter,
    address_str: &str,
) -> Result<ConnectedPeripheral> {
    let target_addr = BDAddr::from_str(address_str)
        .map_err(|e| anyhow!("Invalid Bluetooth address format '{}': {}", address_str, e))?;

    // Find the peripheral in the adapter's known peripherals
    let peripherals = adapter
        .peripherals()
        .await
        .context("Failed to get adapter peripherals")?;

    let peripheral = peripherals
        .into_iter()
        .find(|p| p.address() == target_addr)
        .ok_or_else(|| anyhow!("Peripheral not found: {}", address_str))?;

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
        let uuid = Uuid::parse_str(uuid_str).map_err(|_| anyhow!("Invalid UUID: {}", uuid_str))?;

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
                    bail!(
                        "Failed to write to characteristic {} after 5 attempts: {}",
                        uuid_str,
                        e
                    );
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
    pub async fn power_on(&self, lh: &Lighthouse) -> Result<()> {
        match lh.version() {
            crate::lighthouse::LighthouseVersion::V1 => {
                let id = lh
                    .id
                    .as_ref()
                    .ok_or_else(|| anyhow!("V1 lighthouse missing ID"))?;
                let cmd = protocol::build_v1_power_on(id).map_err(|e| anyhow!("{}", e))?;
                self.write_and_disconnect(V1_POWER_CHARACTERISTIC, &cmd)
                    .await
            }
            crate::lighthouse::LighthouseVersion::V2 => {
                let cmd = protocol::build_v2_power_on();
                self.write_and_disconnect(V2_POWER_CHARACTERISTIC, &cmd)
                    .await
            }
            crate::lighthouse::LighthouseVersion::Unknown => {
                bail!("Cannot power on lighthouse with unknown version");
            }
        }
    }

    /// Sleep the connected lighthouse.
    pub async fn sleep(&self, lh: &Lighthouse) -> Result<()> {
        match lh.version() {
            crate::lighthouse::LighthouseVersion::V1 => {
                let id = lh
                    .id
                    .as_ref()
                    .ok_or_else(|| anyhow!("V1 lighthouse missing ID"))?;
                let cmd = protocol::build_v1_sleep(id).map_err(|e| anyhow!("{}", e))?;
                self.write_and_disconnect(V1_POWER_CHARACTERISTIC, &cmd)
                    .await
            }
            crate::lighthouse::LighthouseVersion::V2 => {
                let cmd = protocol::build_v2_sleep();
                self.write_and_disconnect(V2_POWER_CHARACTERISTIC, &cmd)
                    .await
            }
            crate::lighthouse::LighthouseVersion::Unknown => {
                bail!("Cannot sleep lighthouse with unknown version");
            }
        }
    }

    /// Identify the connected lighthouse (V2 only — causes LED flash).
    pub async fn identify(&self, lh: &Lighthouse) -> Result<()> {
        let _cmd = protocol::build_identify_command(lh).map_err(|e| anyhow!("{}", e))?; // validate first
        let cmd = protocol::build_v2_identify();
        self.write_and_disconnect(V2_IDENTIFY_CHARACTERISTIC, &cmd)
            .await
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

/// Get the first available Bluetooth adapter. Returns an error if none found.
pub async fn get_adapter() -> Result<Adapter> {
    let manager = Manager::new()
        .await
        .map_err(|e| anyhow!("Failed to create BLE manager: {}", e))?;
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

    #[test]
    fn test_bdaddr_from_bytes() {
        let bytes: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let bdaddr: BDAddr = bytes.into();
        assert_eq!(bdaddr.to_string(), "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn test_bdaddr_from_str() {
        let addr_str = "AA:BB:CC:DD:EE:FF";
        let bdaddr = BDAddr::from_str(addr_str).unwrap();
        assert_eq!(bdaddr.to_string(), addr_str);
    }

    #[test]
    fn test_uuid_parse() {
        let uuid = Uuid::parse_str("0000cb01-0000-1000-8000-00805f9b34fb").unwrap();
        assert_eq!(uuid.to_string(), "0000cb01-0000-1000-8000-00805f9b34fb");
    }

    #[test]
    fn test_uuid_constants() {
        assert_eq!(
            V1_POWER_CHARACTERISTIC,
            "0000cb01-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            V2_POWER_CHARACTERISTIC,
            "00001525-1212-efde-1523-785feabcd124"
        );
        assert_eq!(
            V2_IDENTIFY_CHARACTERISTIC,
            "00008421-1212-efde-1523-785feabcd124"
        );
    }
}
