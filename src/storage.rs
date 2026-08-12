use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_aux::prelude::bool_true;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::lighthouse::Lighthouse;

/// Returns the platform-specific local config directory for this app.
///
/// The directory is created if it doesn't already exist.
///
/// # Errors
///
/// Returns an error if the platform-specific config directory cannot be determined.
pub fn config_local_dir() -> Result<PathBuf> {
    let proj = ProjectDirs::from("io", "atomicflag", "Lighthouse Manager")
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    let dir = proj.config_local_dir();
    // Create the directory if it doesn't exist
    fs::create_dir_all(dir).context("Failed to create config directory")?;
    Ok(dir.to_path_buf())
}

/// Path to the JSON settings file, determined cross-platform via `directories`.
fn config_path() -> Result<PathBuf> {
    let dir = config_local_dir()?;
    Ok(dir.join("settings.jsonc"))
}

/// Autostart-related settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Autostart {
    /// Cooldown period in seconds after powering lighthouses off. If `SteamVR` is launched
    /// within this window, lighthouses won't be turned on to avoid frequent toggling.
    pub cooldown_secs: u64,
    /// Unix timestamp (seconds) of when the lighthouses were last turned off.
    pub last_turned_off_at: Option<u64>,
}

impl Default for Autostart {
    fn default() -> Self {
        Self {
            cooldown_secs: 600,
            last_turned_off_at: None,
        }
    }
}

/// Application settings persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub version: u32,
    pub lighthouses: Vec<Lighthouse>,
    #[serde(default)]
    pub autostart: Autostart,
    /// When `true` (the default), power actions run on all managed lighthouses in parallel.
    /// When `false`, they run sequentially.
    #[serde(default = "bool_true")]
    pub parallel_power: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: 1,
            lighthouses: Vec::new(),
            autostart: Autostart::default(),
            parallel_power: true,
        }
    }
}

/// Load settings from a specific path. Returns defaults if file doesn't exist.
fn load_at(path: &PathBuf) -> Result<AppSettings> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let content = fs::read_to_string(path).context("Failed to read settings")?;
    let settings: AppSettings =
        jsonc_parser::parse_to_serde_value(&content, &jsonc_parser::ParseOptions::default())
            .context("Failed to parse settings JSONC")?;
    Ok(settings)
}

/// Save settings to a specific path.
fn save_at(path: &PathBuf, settings: &AppSettings) -> Result<()> {
    let content = serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;
    fs::write(path, content).context("Failed to write settings")?;
    Ok(())
}

/// Load settings from disk (using the default config path). Returns defaults if file doesn't exist.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined.
pub fn load() -> Result<AppSettings> {
    let path = config_path()?;
    load_at(&path)
}

/// Save settings to disk (using the default config path).
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined, the JSON cannot be serialized,
/// or the file cannot be written.
///
/// Settings are written as plain JSON (a valid subset of JSONC); any existing comments
/// in the file are not preserved on save.
pub fn save(settings: &AppSettings) -> Result<()> {
    let path = config_path()?;
    save_at(&path, settings)
}

/// Add newly discovered lighthouses to the settings.
/// - Newly discovered units are marked unmanaged (managed: false) by default.
/// - Deduplication by Bluetooth address: if an entry already exists for this address, it is NOT overwritten.
/// - Returns the count of new entries added.
pub fn add_new(settings: &mut AppSettings, discovered: &[Lighthouse]) -> usize {
    let existing: HashSet<String> = settings
        .lighthouses
        .iter()
        .map(|l| l.address.clone())
        .collect();

    let new_lhs: Vec<Lighthouse> = discovered
        .iter()
        .filter(|lh| !existing.contains(&lh.address))
        .cloned()
        .collect();
    let count = new_lhs.len();
    settings.lighthouses.extend(new_lhs);
    count
}

/// Get all managed lighthouses.
#[must_use]
pub fn managed_lighthouses(settings: &AppSettings) -> Vec<&Lighthouse> {
    settings.lighthouses.iter().filter(|l| l.managed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_settings_path() -> (PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.jsonc");
        (path, dir)
    }

    #[test]
    fn test_load_empty_settings() {
        let (path, _guard) = test_settings_path();
        let settings = load_at(&path).unwrap();
        assert!(settings.lighthouses.is_empty());
        assert_eq!(settings.version, 1);
    }

    #[test]
    fn test_save_and_load() {
        let (path, _guard) = test_settings_path();

        let settings = AppSettings {
            version: 1,
            lighthouses: vec![Lighthouse {
                name: "LHB-0A1B2C3D".into(),
                address: "AA:BB:CC:DD:EE:FF".into(),
                id: None,
                managed: true,
            }],
            ..Default::default()
        };
        save_at(&path, &settings).unwrap();

        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.lighthouses.len(), 1);
        assert_eq!(loaded.lighthouses[0].name, "LHB-0A1B2C3D");
        assert!(loaded.lighthouses[0].managed);
    }

    #[test]
    fn test_add_new_deduplication() {
        let (path, _guard) = test_settings_path();

        let mut settings = AppSettings {
            version: 1,
            lighthouses: vec![Lighthouse {
                name: "HTC BS-AABBCCDD".into(),
                address: "AA:BB:CC:DD:EE:FF".into(),
                id: Some("AABBCCDD".into()),
                managed: true,
            }],
            ..Default::default()
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

        let count = add_new(&mut settings, &discovered);
        assert_eq!(count, 1); // Only the new address was added
        assert_eq!(settings.lighthouses.len(), 2);
        // Original entry preserved (not overwritten by discovered)
        assert_eq!(settings.lighthouses[0].name, "HTC BS-AABBCCDD");

        save_at(&path, &settings).ok();
    }

    #[test]
    fn test_newly_discovered_are_unmanaged() {
        let (path, _guard) = test_settings_path();

        let mut settings = AppSettings::default();
        let discovered = vec![Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: None,
            managed: false, // BLE scan always produces unmanaged lighthouses
        }];

        add_new(&mut settings, &discovered);

        assert!(!settings.lighthouses[0].managed);

        save_at(&path, &settings).ok();
    }

    #[test]
    fn test_managed_lighthouses_filter() {
        let settings = AppSettings {
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
            ..Default::default()
        };

        let managed = managed_lighthouses(&settings);
        assert_eq!(managed.len(), 2);
        assert_eq!(managed[0].name, "LHB-0000");
        assert_eq!(managed[1].name, "LHB-2222");
    }

    #[test]
    fn test_serde_roundtrip() {
        let settings = AppSettings {
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
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: AppSettings =
            jsonc_parser::parse_to_serde_value(&json, &jsonc_parser::ParseOptions::default())
                .unwrap();
        assert_eq!(restored.version, 1);
        assert_eq!(restored.lighthouses.len(), 2);
    }
}
