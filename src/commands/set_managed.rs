use anyhow::{bail, Result};
use tracing::info;

use crate::storage;

/// Represents either a single lighthouse index or a request to target all entries.
enum IndexOrAll {
    All,
    Index(usize),
}

/// Parse the target argument: "all" for every lighthouse, or a numeric index.
fn parse_target(target: &str, db_len: usize) -> Result<IndexOrAll> {
    if target == "all" {
        return Ok(IndexOrAll::All);
    }

    let idx: usize = target
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid index '{target}'. Use a number or 'all'."))?;

    if idx >= db_len {
        bail!(
            "Index {idx} out of range (database has {db_len} entries). Use `lighthouse-manager list` to see available indices."
        );
    }

    Ok(IndexOrAll::Index(idx))
}

/// Set the managed flag on one or all lighthouses.
///
/// # Errors
///
/// Returns an error if the database cannot be loaded, the target index is invalid,
/// or the database cannot be saved.
pub fn run(target: &str, managed: bool) -> Result<()> {
    let mut db = storage::load()?;

    if db.lighthouses.is_empty() {
        bail!(
            "No lighthouses in the database. Run `lighthouse-manager discover` to scan for nearby devices."
        );
    }

    let label = if managed { "managed" } else { "unmanaged" };
    let target = parse_target(target, db.lighthouses.len())?;

    match target {
        IndexOrAll::All => {
            for lh in &mut db.lighthouses {
                lh.managed = managed;
            }
            info!(
                count = db.lighthouses.len(),
                label,
                "Set {} lighthouse(s) as {}",
                db.lighthouses.len(),
                label
            );
        }
        IndexOrAll::Index(idx) => {
            db.lighthouses[idx].managed = managed;
            let name = db.lighthouses[idx].name.as_str();
            info!(
                index = idx,
                name = %name,
                "Set '{name}' at index [{idx}] as {label}"
            );
        }
    }

    storage::save(&db)?;

    Ok(())
}
