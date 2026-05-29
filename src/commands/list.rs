use anyhow::Result;
use tracing::{debug, info};

use crate::storage;

/// List all known lighthouses or only managed ones.
///
/// # Errors
///
/// Returns an error if the database cannot be loaded or JSON serialization fails.
pub fn run(managed_only: bool, json_output: bool) -> Result<()> {
    let db = storage::load()?;

    if db.lighthouses.is_empty() {
        info!(
            "No lighthouses in the database. Run `lighthouse-manager discover` to scan for nearby devices."
        );
        return Ok(());
    }

    let filtered: Vec<_> = db
        .lighthouses
        .iter()
        .filter(|l| !managed_only || l.managed)
        .collect();

    if filtered.is_empty() {
        if managed_only {
            info!("No managed lighthouses found.");
        }
        return Ok(());
    }

    // JSON output
    if json_output {
        let json = serde_json::to_string_pretty(&filtered)?;
        debug!("{}", json);
        return Ok(());
    }

    info!("Known lighthouses ({}):", filtered.len());
    for (i, lh) in filtered.iter().enumerate() {
        info!(index = i, name = %lh.name, address = %lh.address, version = %lh.version(), managed = lh.managed, "found");
        if let Some(id) = &lh.id {
            info!(index = i, id = %id, "V1 lighthouse ID");
        }
    }

    info!("Use `lighthouse-manager list --managed` to view only managed stations.");

    let managed_count = filtered.iter().filter(|l| l.managed).count();
    if managed_count > 0 {
        info!(
            managed = managed_count,
            total = filtered.len(),
            "Managed lighthouses count"
        );
    }

    Ok(())
}
