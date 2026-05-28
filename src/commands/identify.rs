use anyhow::{Context, Result, bail};
use btleplug::api::{Central, Peripheral};
use std::collections::HashSet;
use std::time::Duration;
use tracing::{error, info};

use crate::lighthouse::LighthouseVersion;

/// Identify a specific V2 lighthouse by index in the database.
/// This causes the lighthouse to blink its LED for visual identification.
pub async fn run(index: usize) -> Result<()> {
    let db = crate::storage::load()?;

    if index >= db.lighthouses.len() {
        anyhow::bail!(
            "Index {} out of range (database has {} entries). Use `lighthouse-manager list` to see available indices.",
            index,
            db.lighthouses.len()
        );
    }

    let lh = &db.lighthouses[index];

    if lh.version() != LighthouseVersion::V2 {
        anyhow::bail!(
            "Identify is only supported on V2 lighthouses. '{}' is a {}",
            lh.name,
            lh.version()
        );
    }

    info!(
        "Sending identify command to {} ({}), index [{}]...",
        lh.name, lh.address, index
    );

    let adapter = crate::bluetooth::get_adapter().await?;

    // Start a BLE scan so that BlueZ begins advertising discovered devices.
    adapter
        .start_scan(btleplug::api::ScanFilter { services: vec![] })
        .await
        .context("Failed to start BLE scan")?;

    // Wait until this lighthouse has been observed (or timeout).
    let poll_interval = Duration::from_millis(500);
    let timeout = Duration::from_secs(15);
    let target_addr = lh.address.to_lowercase();
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

            if discovered.contains(&target_addr) {
                // Target lighthouse has been observed — proceed.
                break;
            }
        }

        tokio::time::sleep(poll_interval).await;
    }

    adapter.stop_scan().await.ok();

    // Verify the device was actually found.
    let found = if let Ok(peripherals) = adapter.peripherals().await {
        peripherals
            .iter()
            .any(|p| p.address().to_string().to_lowercase() == target_addr)
    } else {
        false
    };

    if !found {
        bail!(
            "Could not observe {} ({}). Make sure it is nearby and powered on.",
            lh.name,
            lh.address
        );
    }

    match tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let conn = crate::bluetooth::connect_lighthouse(&adapter, &lh.address).await?;
        conn.identify(lh).await?;
        conn.disconnect().await;
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => {
            info!("✓ {} should be blinking now.", lh.name);
        }
        Ok(Err(e)) => {
            error!("Failed to identify {}: {}", lh.name, e);
            return Err(e);
        }
        Err(_) => {
            anyhow::bail!(
                "Timed out while trying to identify {}. Is it powered on and nearby?",
                lh.name
            );
        }
    }

    Ok(())
}
