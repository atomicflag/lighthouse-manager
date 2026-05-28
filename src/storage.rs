use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::lighthouse::Lighthouse;

/// Path to the JSON database file, determined cross-platform via `directories`.
fn config_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("io", "atomicflag", "Lighthouse Manager")
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    let dir = proj.config_local_dir();
    // Create the directory if it doesn't exist
    fs::create_dir_all(dir).context("Failed to create config directory")?;
    Ok(dir.join("lighthouses.json"))
}

/// JSON database containing all known lighthouses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighthouseDatabase {
    pub version: u32,
    pub lighthouses: Vec<Lighthouse>,
}

impl Default for LighthouseDatabase {
    fn default() -> Self {
        Self {
            version: 1,
            lighthouses: Vec::new(),
        }
    }
}

/// Load the database from a specific path. Returns empty DB if file doesn't exist.
fn load_at(path: &PathBuf) -> Result<LighthouseDatabase> {
    if !path.exists() {
        return Ok(LighthouseDatabase::default());
    }
    let content = fs::read_to_string(path).context("Failed to read lighthouse database")?;
    let db: LighthouseDatabase =
        serde_json::from_str(&content).context("Failed to parse lighthouse database JSON")?;
    Ok(db)
}

/// Save the database to a specific path.
fn save_at(path: &PathBuf, db: &LighthouseDatabase) -> Result<()> {
    let content = serde_json::to_string_pretty(db).context("Failed to serialize database")?;
    fs::write(path, content).context("Failed to write lighthouse database")?;
    Ok(())
}

/// Load the database from disk (using the default config path). Returns empty DB if file doesn't exist.
pub fn load() -> Result<LighthouseDatabase> {
    let path = config_path()?;
    load_at(&path)
}

/// Save the database to disk (using the default config path).
pub fn save(db: &LighthouseDatabase) -> Result<()> {
    let path = config_path()?;
    save_at(&path, db)
}

/// Add newly discovered lighthouses to the database.
/// - Newly discovered units are marked unmanaged (managed: false) by default.
/// - Deduplication by Bluetooth address: if an entry already exists for this address, it is NOT overwritten.
/// - Returns the count of new entries added.
pub fn add_new(db: &mut LighthouseDatabase, discovered: &[Lighthouse]) -> usize {
    let mut count = 0;
    // Clone addresses to avoid borrowing db.lighthouses immutably while pushing
    let existing_addresses: Vec<String> =
        db.lighthouses.iter().map(|l| l.address.clone()).collect();

    for lh in discovered {
        if !existing_addresses.contains(&lh.address) {
            // Mark newly discovered units as unmanaged by default
            db.lighthouses.push(Lighthouse {
                name: lh.name.clone(),
                address: lh.address.clone(),
                id: lh.id.clone(),
                managed: false,
            });
            count += 1;
        }
    }
    count
}

/// Get all managed lighthouses.
pub fn managed_lighthouses(db: &LighthouseDatabase) -> Vec<&Lighthouse> {
    db.lighthouses.iter().filter(|l| l.managed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_db_path() -> (PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lighthouses.json");
        (path, dir)
    }

    #[test]
    fn test_load_empty_db() {
        let (path, _guard) = test_db_path();
        let db = load_at(&path).unwrap();
        assert!(db.lighthouses.is_empty());
        assert_eq!(db.version, 1);
    }

    #[test]
    fn test_save_and_load() {
        let (path, _guard) = test_db_path();

        let db = LighthouseDatabase {
            version: 1,
            lighthouses: vec![Lighthouse {
                name: "LHB-0A1B2C3D".into(),
                address: "AA:BB:CC:DD:EE:FF".into(),
                id: None,
                managed: true,
            }],
        };
        save_at(&path, &db).unwrap();

        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.lighthouses.len(), 1);
        assert_eq!(loaded.lighthouses[0].name, "LHB-0A1B2C3D");
        assert!(loaded.lighthouses[0].managed);
    }

    #[test]
    fn test_add_new_deduplication() {
        let (path, _guard) = test_db_path();

        let mut db = LighthouseDatabase {
            version: 1,
            lighthouses: vec![Lighthouse {
                name: "HTC BS-AABBCCDD".into(),
                address: "AA:BB:CC:DD:EE:FF".into(),
                id: Some("AABBCCDD".into()),
                managed: true,
            }],
        };

        // Discover same device (should be deduplicated) and a new one
        let discovered = vec![
            Lighthouse {
                name: "HTC BS-AABBCCDD-NEW".into(), // Same address, different name
                address: "AA:BB:CC:DD:EE:FF".into(),
                id: Some("AABBCCDD2".into()),
                managed: true,
            },
            Lighthouse {
                name: "LHB-0A1B2C3D".into(),
                address: "11:22:33:44:55:66".into(),
                id: None,
                managed: true,
            },
        ];

        let count = add_new(&mut db, &discovered);
        assert_eq!(count, 1); // Only the new address was added
        assert_eq!(db.lighthouses.len(), 2);
        // Original entry preserved (not overwritten by discovered)
        assert_eq!(db.lighthouses[0].name, "HTC BS-AABBCCDD");

        save_at(&path, &db).ok();
    }

    #[test]
    fn test_newly_discovered_are_unmanaged() {
        let (path, _guard) = test_db_path();

        let mut db = LighthouseDatabase::default();
        let discovered = vec![Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: None,
            managed: true, // Discoverer might think it's managed, but add_new overrides
        }];

        add_new(&mut db, &discovered);

        // Newly added should be unmanaged regardless of what was passed in
        assert!(!db.lighthouses[0].managed);

        save_at(&path, &db).ok();
    }

    #[test]
    fn test_managed_lighthouses_filter() {
        let db = LighthouseDatabase {
            version: 1,
            lighthouses: vec![
                Lighthouse {
                    name: "LHB-0000".into(),
                    address: "AA:00".into(),
                    id: None,
                    managed: true,
                },
                Lighthouse {
                    name: "HTC BS-1111".into(),
                    address: "BB:00".into(),
                    id: Some("1111".into()),
                    managed: false,
                },
                Lighthouse {
                    name: "LHB-2222".into(),
                    address: "CC:00".into(),
                    id: None,
                    managed: true,
                },
            ],
        };

        let managed = managed_lighthouses(&db);
        assert_eq!(managed.len(), 2);
        assert_eq!(managed[0].name, "LHB-0000");
        assert_eq!(managed[1].name, "LHB-2222");
    }

    #[test]
    fn test_serde_roundtrip_database() {
        let db = LighthouseDatabase {
            version: 1,
            lighthouses: vec![
                Lighthouse {
                    name: "HTC BS-AABBCCDD".into(),
                    address: "AA:BB:CC:DD:EE:FF".into(),
                    id: Some("AABBCCDD".into()),
                    managed: true,
                },
                Lighthouse {
                    name: "LHB-0A1B2C3D".into(),
                    address: "11:22:33:44:55:66".into(),
                    id: None,
                    managed: false,
                },
            ],
        };

        let json = serde_json::to_string_pretty(&db).unwrap();
        let restored: LighthouseDatabase = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.version, 1);
        assert_eq!(restored.lighthouses.len(), 2);
    }
}
