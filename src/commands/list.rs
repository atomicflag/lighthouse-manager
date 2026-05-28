use anyhow::Result;
use tracing::{debug, info};

use crate::lighthouse::Lighthouse;
use crate::storage;

/// List all known lighthouses or only managed ones.
pub fn run(managed_only: bool, json_output: bool) -> Result<()> {
    let db = storage::load()?;

    if db.lighthouses.is_empty() {
        info!(
            "No lighthouses in the database. Run `lighthouse-manager discover` to scan for nearby devices."
        );
        return Ok(());
    }

    let all: Vec<&Lighthouse> = db.lighthouses.iter().collect();
    let filtered: Vec<&Lighthouse> = if managed_only {
        all.into_iter().filter(|l| l.managed).collect()
    } else {
        all
    };

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
        info!(
            index = i,
            name = lh.name.as_str(),
            address = lh.address.as_str(),
            version = %lh.version(),
            managed = lh.managed,
            "found"
        );
        if let Some(id) = &lh.id {
            info!(index = i, id = id.as_str(), "V1 lighthouse ID");
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
