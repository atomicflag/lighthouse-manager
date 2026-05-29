use anyhow::{Result, bail};
use std::collections::HashSet;
use tracing::{error, info};

use crate::bluetooth;
use crate::lighthouse::Lighthouse;
use crate::storage;

/// Power on all managed lighthouses in parallel.
///
/// # Errors
///
/// Returns an error if no Bluetooth adapter is available, managed devices cannot be discovered,
/// or any power-on command fails.
pub async fn power_on() -> Result<()> {
    run_power_action(PowerAction::PowerOn).await
}

/// Sleep (power off) all managed lighthouses in parallel.
///
/// # Errors
///
/// Returns an error if no Bluetooth adapter is available, managed devices cannot be discovered,
/// or any sleep command fails.
pub async fn power_off() -> Result<()> {
    run_power_action(PowerAction::Sleep).await
}

#[derive(Debug, Clone, Copy)]
enum PowerAction {
    PowerOn,
    Sleep,
}

/// Load the database, collect managed lighthouses, and validate they are non-empty.
pub(super) fn load_and_validate() -> Result<Vec<Lighthouse>> {
    let db = storage::load()?;
    let managed: Vec<Lighthouse> = storage::managed_lighthouses(&db)
        .into_iter()
        .cloned()
        .collect();

    if managed.is_empty() {
        info!("No managed lighthouses found. Mark stations as managed in the database first.");
        return Ok(managed);
    }

    Ok(managed)
}

/// Execute a power action on all managed lighthouses in parallel.
async fn run_power_action(action: PowerAction) -> Result<()> {
    let managed = load_and_validate()?;

    if managed.is_empty() {
        return Ok(());
    }

    info!(
        action = ?action,
        count = managed.len(),
        "Power action on managed lighthouses"
    );

    let expected_addresses: HashSet<String> =
        managed.iter().map(|m| m.address.to_lowercase()).collect();

    let adapter = bluetooth::get_adapter().await?;

    let discovered = bluetooth::scan_until_predicate(&adapter, |discovered| {
        expected_addresses.is_subset(discovered)
    })
    .await?;

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

    execute_and_report(&adapter, managed, action).await?;

    Ok(())
}

/// Spawn parallel tasks for each lighthouse, join results, and report.
async fn execute_and_report(
    adapter: &bluetooth::Adapter,
    managed: Vec<Lighthouse>,
    action: PowerAction,
) -> Result<()> {
    let mut tasks = Vec::new();

    for lh in &managed {
        let adapter_clone = adapter.clone();
        let lh_for_task = lh.clone();

        let task =
            tokio::spawn(async move { send_action(&adapter_clone, &lh_for_task, action).await });

        tasks.push(task);
    }

    let results = futures::future::join_all(tasks).await;

    let mut error_count = 0;

    for (i, result) in results.into_iter().enumerate() {
        if i >= managed.len() {
            continue;
        }

        match result {
            Ok(Ok(())) => {
                info!(device = %managed[i].name, "power action complete");
            }
            Ok(Err(e)) => {
                error_count += 1;
                error!(device = %managed[i].name, error = %e, "power action failed");
            }
            Err(join_err) => {
                error_count += 1;
                error!(device = %managed[i].name, task_error = %join_err, "task panicked or was cancelled");
            }
        }
    }

    let action_display = match &action {
        PowerAction::PowerOn => "ON",
        PowerAction::Sleep => "OFF",
    };

    info!(
        action = ?action,
        success = managed.len() - error_count,
        failed = error_count,
        total = managed.len(),
        power_action = action_display,
        "power action complete"
    );

    if error_count > 0 {
        bail!("Some devices failed to respond. Check connections and IDs.");
    }

    Ok(())
}

/// Connect to a single lighthouse and send the specified power action.
async fn send_action(
    adapter: &crate::bluetooth::Adapter,
    lh: &Lighthouse,
    action: PowerAction,
) -> Result<()> {
    let conn = crate::bluetooth::connect_lighthouse(adapter, &lh.address).await?;
    match action {
        PowerAction::PowerOn => conn.power_on(lh).await?,
        PowerAction::Sleep => conn.sleep(lh).await?,
    }
    conn.disconnect().await;
    Ok(())
}
