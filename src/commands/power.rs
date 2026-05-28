use anyhow::{Context, Result, bail};
use btleplug::api::{Central, Peripheral};
use std::collections::HashSet;
use std::time::Duration;
use tracing::info;

use crate::bluetooth;
use crate::lighthouse::Lighthouse;
use crate::storage;

/// Power on all managed lighthouses in parallel.
pub async fn power_on() -> Result<()> {
    run_power_action(PowerAction::PowerOn).await
}

/// Sleep (power off) all managed lighthouses in parallel.
pub async fn power_off() -> Result<()> {
    run_power_action(PowerAction::Sleep).await
}

#[derive(Debug, Clone)]
enum PowerAction {
    PowerOn,
    Sleep,
}

/// Execute a power action on all managed lighthouses in parallel.
async fn run_power_action(action: PowerAction) -> Result<()> {
    let db = storage::load()?;
    let managed: Vec<Lighthouse> = storage::managed_lighthouses(&db)
        .into_iter()
        .cloned()
        .collect();

    if managed.is_empty() {
        println!("No managed lighthouses found. Mark stations as managed in the database first.");
        return Ok(());
    }

    let action_for_display = action.clone();
    let names: Vec<String> = managed.iter().map(|m| m.name.clone()).collect();

    info!(
        "Power {:?} on {} managed lighthouse(s)...",
        action,
        managed.len()
    );

    // Build a set of expected MAC addresses so we can check whether each
    // device has been observed yet.
    let expected_addresses: HashSet<String> =
        managed.iter().map(|m| m.address.to_lowercase()).collect();

    let adapter = bluetooth::get_adapter().await?;

    // Start a BLE scan so that BlueZ begins advertising discovered devices.
    adapter
        .start_scan(btleplug::api::ScanFilter { services: vec![] })
        .await
        .context("Failed to start BLE scan")?;

    // Wait until every managed lighthouse has been observed (or timeout).
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

            if expected_addresses.is_subset(&discovered) {
                // All managed lighthouses have been observed — proceed.
                break;
            }
        }

        tokio::time::sleep(poll_interval).await;
    }

    adapter.stop_scan().await.ok();

    // After scanning, take a final snapshot of discovered peripherals to check
    // which expected devices were actually found.
    let discovered: HashSet<String> = if let Ok(peripherals) = adapter.peripherals().await {
        peripherals
            .iter()
            .map(|p| p.address().to_string().to_lowercase())
            .collect()
    } else {
        HashSet::new()
    };

    let missing: Vec<String> = expected_addresses
        .difference(&discovered)
        .cloned()
        .collect();

    if !missing.is_empty() {
        bail!(
            "Could not observe {} of {} managed lighthouse(s): {}. Check that they are nearby and powered on.",
            missing.len(),
            managed.len(),
            missing.join(", ")
        );
    }

    // Spawn a task for each managed lighthouse for parallel control.
    // Each task independently connects, sends command, and disconnects.
    let mut tasks = Vec::new();

    for lh in managed {
        let adapter_clone = adapter.clone();
        let name = lh.name.clone();
        let action_clone = action.clone();

        let task = tokio::spawn(async move {
            match action_clone {
                PowerAction::PowerOn => send_power_on(&adapter_clone, &lh).await,
                PowerAction::Sleep => send_sleep(&adapter_clone, &lh).await,
            }
            .map(|_| name.clone())
        });

        tasks.push(task);
    }

    // Await all tasks concurrently - fully parallel like Task.WhenAll
    let results = futures::future::join_all(tasks).await;

    // Process results
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, result) in results.into_iter().enumerate() {
        if i >= names.len() {
            continue;
        }

        match result {
            Ok(Ok(name)) => {
                info!("✓ {}: power action complete", name);
                success_count += 1;
            }
            Ok(Err(e)) => {
                error_count += 1;
                eprintln!("  [{}]: {}", names[i], e);
            }
            Err(join_err) => {
                error_count += 1;
                eprintln!(
                    "  [{}]: Task panicked or was cancelled: {}",
                    names[i], join_err
                );
            }
        }
    }

    println!(
        "\nPower {:?} complete: {} succeeded, {} failed out of {} managed lighthouse(s).",
        action_for_display,
        success_count,
        error_count,
        names.len()
    );

    if error_count > 0 {
        anyhow::bail!("Some devices failed to respond. Check connections and IDs.");
    }

    Ok(())
}

/// Connect to a single lighthouse and send power-on command based on version.
async fn send_power_on(adapter: &crate::bluetooth::Adapter, lh: &Lighthouse) -> Result<()> {
    let conn = crate::bluetooth::connect_lighthouse(adapter, &lh.address).await?;
    conn.power_on(lh).await?;
    conn.disconnect().await;
    Ok(())
}

/// Connect to a single lighthouse and send sleep command based on version.
async fn send_sleep(adapter: &crate::bluetooth::Adapter, lh: &Lighthouse) -> Result<()> {
    let conn = crate::bluetooth::connect_lighthouse(adapter, &lh.address).await?;
    conn.sleep(lh).await?;
    conn.disconnect().await;
    Ok(())
}
