use anyhow::Result;
use tracing::info;

use crate::storage;

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

    // Try to fill in V1 IDs from existing entries and name parsing
    for lh in &discovered {
        // If we already have an entry in the DB, borrow its ID
        if let Some(existing) = db.lighthouses.iter().find(|e| e.address == lh.address)
            && existing.id.is_some()
            && lh.id.is_none()
        {
            // We'll update via add_new logic instead
        }
    }

    storage::save(&db)?;

    info!(
        "Discovered {} Lighthouse(s), {} new entry(ies) saved.",
        discovered.len(),
        new_count
    );

    println!("\nLighthouses found:");
    for lh in &discovered {
        let managed_mark = if lh.managed { "[M]" } else { "   " };
        let id_info = match (lh.version().is_v1(), &lh.id) {
            (true, Some(id)) => format!(" (ID: {})", id),
            (true, None) => " (⚠ missing ID - edit config)".to_string(),
            _ => String::new(),
        };
        println!(
            "  {} {:30} {}{}",
            managed_mark, lh.name, lh.address, id_info
        );
    }

    if new_count > 0 {
        info!("Use `lighthouse-manager list --managed` to see managed lighthouses.");
        info!(
            "Edit the database file to mark stations as managed: {}",
            db_path()
        );
    }

    Ok(())
}

fn db_path() -> String {
    if let Some(proj) = directories::ProjectDirs::from("io", "atomicflag", "Lighthouse Manager") {
        proj.config_local_dir()
            .join("lighthouses.json")
            .display()
            .to_string()
    } else {
        "~/.config/lighthouse-manager/lighthouses.json".to_string()
    }
}
