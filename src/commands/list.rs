use anyhow::Result;

use crate::lighthouse::{Lighthouse, LighthouseVersion};
use crate::storage;

/// List all known lighthouses or only managed ones.
pub fn run(managed_only: bool, json_output: bool) -> Result<()> {
    let db = storage::load()?;

    if db.lighthouses.is_empty() {
        println!(
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
            println!("No managed lighthouses found.");
        }
        return Ok(());
    }

    // JSON output
    if json_output {
        let json = serde_json::to_string_pretty(&filtered)?;
        println!("{}", json);
        return Ok(());
    }

    // Table output
    println!("\nKnown Lighthouses ({}):", filtered.len());
    println!("{:<5} {:30} {:20} {:6} ID", "IDX", "Name", "Address", "V");
    println!("{}", "-".repeat(80));

    for (i, lh) in filtered.iter().enumerate() {
        let idx = format!("[{}]", i);
        let name = if lh.managed {
            format!(" [M] {}", lh.name)
        } else {
            format!("     {}", lh.name)
        };
        let version = match lh.version() {
            crate::lighthouse::LighthouseVersion::V1 => "V1".to_string(),
            crate::lighthouse::LighthouseVersion::V2 => "V2".to_string(),
        };
        let id = match &lh.id {
            Some(id) if lh.version() == LighthouseVersion::V1 => id.clone(),
            _ => String::new(),
        };
        println!(
            "{:<5} {:30} {:20} {:6} {}",
            idx, name, lh.address, version, id
        );
    }

    println!("\n[M] = Managed (will be controlled by power commands)");
    println!("To manage a station, edit the database file.");

    // Show managed count separately
    let managed_count = filtered.iter().filter(|l| l.managed).count();
    if managed_count > 0 {
        println!("\nManaged: {} of {} listed.", managed_count, filtered.len());
    }

    Ok(())
}
