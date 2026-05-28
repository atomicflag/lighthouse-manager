use anyhow::Result;
use tracing::info;

use crate::{lighthouse::LighthouseVersion, storage};

/// Discover nearby lighthouses and save them to the database.
/// Newly discovered units are added unmanaged; existing entries are preserved (dedup by address).
pub async fn run(timeout_secs: u64) -> Result<()> {
    let adapter = crate::bluetooth::get_adapter().await?;

    let discovered = crate::bluetooth::discover_lighthouses(&adapter, timeout_secs).await?;

    if discovered.is_empty() {
        info!("No Lighthouse base stations found.");
        return Ok(());
    }

    // Load the database (creates it if doesn't exist)
    let mut db = storage::load()?;
    let new_count = storage::add_new(&mut db, &discovered);

    storage::save(&db)?;

    info!(
        discovered = discovered.len(),
        new_entries = new_count,
        "discovery complete"
    );

    info!("Lighthouses found:");
    for lh in &discovered {
        info!(name = %lh.name, address = %lh.address, managed = lh.managed, "found");
        let id_info = match (lh.version() == LighthouseVersion::V1, &lh.id) {
            (true, Some(id)) => format!(" V1 ID: {id}"),
            (true, None) => " WARNING: missing V1 ID - edit config".to_string(),
            _ => String::new(),
        };
        if !id_info.is_empty() {
            info!(device = %lh.name, details = %id_info);
        }
    }

    if new_count > 0 {
        info!("Use `lighthouse-manager list --managed` to see managed lighthouses.");
        info!("Edit the database file to mark stations as managed.");
    }

    Ok(())
}
