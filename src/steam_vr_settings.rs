use std::path::PathBuf;

use serde::Deserialize;
use tokio::io;

/// Representation of the relevant ``SteamVR`` Settings
#[derive(Deserialize)]
pub struct VrSettings {
    driver_lighthouse: Option<DriverLighthouse>,
}

/// Contains the full IDs of the light houses
#[derive(Deserialize)]
struct DriverLighthouse {
    #[serde(rename = "PowerManagedBaseStations2")]
    power_managed_base_stations2: Option<String>,
}

impl VrSettings {
    /// Given the name of a lighthouse obtain the full ID if listed in the settings
    #[must_use]
    pub fn get_id_by_name(&self, name: &str) -> Option<String> {
        let lighthouses = self
            .driver_lighthouse
            .as_ref()?
            .power_managed_base_stations2
            .as_ref()?;
        let id_suffix = name.get(name.len().checked_sub(4)?..)?;
        let mut ids = lighthouses.split(',').filter_map(|entry| {
            let serial = entry.split(':').next()?.trim();
            let id = serial.strip_prefix("LHB-")?;
            let tail = id.get(id.len().checked_sub(4)?..)?;
            tail.eq_ignore_ascii_case(id_suffix).then(|| id.to_string())
        });
        let id = ids.next();
        if ids.next().is_some() { None } else { id }
    }
}

const DEFAULT_RELATIVE_PATH: &str = ".steam/steam/config/steamvr.vrsettings";

/// Get the path where the ``SteamVR`` settings are loaded from.
#[must_use]
pub fn get_vr_settings_path() -> PathBuf {
    if let Some(path) = std::env::var_os("STEAM_VR_SETTINGS_PATH") {
        return PathBuf::from(path);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(DEFAULT_RELATIVE_PATH),
        None => PathBuf::from(DEFAULT_RELATIVE_PATH),
    }
}

/// Load the ``SteamVR`` settings. This file is created automatically upon starting ``SteamVR``.
///
/// # Errors
///
/// Returns an error if loading the file fails
pub async fn load_vr_settings() -> io::Result<String> {
    let path = get_vr_settings_path();
    tokio::fs::read_to_string(path).await
}

/// Parse the ``SteamVR`` Settings JSON String
///
/// # Errors
///
/// Returns an error if the JSON cannot be parsed
pub fn parse_vr_settings(json: &str) -> serde_json::Result<VrSettings> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(list: &str) -> VrSettings {
        parse_vr_settings(&format!(
            r#"{{ "driver_lighthouse": {{ "PowerManagedBaseStations2": "{list}" }} }}"#
        ))
        .unwrap()
    }

    #[test]
    fn finds_id_by_name() {
        let s = settings("LHB-AAAA1111:BS1:1,LHB-BBBB2222:BS1:1");
        assert_eq!(
            s.get_id_by_name("HTC BS 111111").as_deref(),
            Some("AAAA1111")
        );
        assert_eq!(
            s.get_id_by_name("HTC BS 222222").as_deref(),
            Some("BBBB2222")
        );
    }

    #[test]
    fn unknown_station_returns_none() {
        let s = settings("LHB-AAAA1111:BS1:1");
        assert_eq!(s.get_id_by_name("HTC BS 222222"), None);
    }

    #[test]
    fn missing_section_returns_none() {
        let s = parse_vr_settings(r#"{ "steamvr": {} }"#).unwrap();
        assert_eq!(s.get_id_by_name("HTC BS 111111"), None);
    }

    #[test]
    fn short_name_does_not_panic() {
        let s = settings("LHB-AAAA1111:BS1:1");
        assert_eq!(s.get_id_by_name("abc"), None);
    }
}
