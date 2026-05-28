use serde::{Deserialize, Serialize};
use std::fmt;

/// Version of a Lighthouse base station.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LighthouseVersion {
    Unknown,
    V1, // HTC BS-*
    V2, // LHB-*
}

impl LighthouseVersion {
    pub fn is_v1(&self) -> bool {
        matches!(self, LighthouseVersion::V1)
    }
    pub fn is_v2(&self) -> bool {
        matches!(self, LighthouseVersion::V2)
    }
}

impl fmt::Display for LighthouseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LighthouseVersion::Unknown => write!(f, "Unknown"),
            LighthouseVersion::V1 => write!(f, "V1 (HTC BS)"),
            LighthouseVersion::V2 => write!(f, "V2 (LHB)"),
        }
    }
}

/// A discovered or known Lighthouse base station.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lighthouse {
    /// Friendly name of the device (e.g. "HTC BS-AABBCCDD" or "LHB-0A1B2C3D").
    pub name: String,

    /// Bluetooth MAC address in colon-separated hex format: "AA:BB:CC:DD:EE:FF".
    pub address: String,

    /// 8-char hex ID printed on the back of V1 lighthouses (e.g. "AABBCCDD").
    /// Only present for V1 devices and required for power control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Whether this lighthouse should be controlled by this manager.
    /// Newly discovered lighthouses are unmanaged by default.
    #[serde(default)]
    pub managed: bool,
}

impl Lighthouse {
    /// Determine the version of a lighthouse from its name.
    /// - Starts with "HTC BS" → V1
    /// - Starts with "LHB-"   → V2
    /// - Otherwise            → Unknown
    pub fn version(&self) -> LighthouseVersion {
        if self.name.starts_with("HTC BS") {
            LighthouseVersion::V1
        } else if self.name.starts_with("LHB-") {
            LighthouseVersion::V2
        } else {
            LighthouseVersion::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v1_version_detection() {
        let lh = Lighthouse {
            name: "HTC BS-AABBCCDD".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: None,
            managed: false,
        };
        assert_eq!(lh.version(), LighthouseVersion::V1);
    }

    #[test]
    fn test_v2_version_detection() {
        let lh = Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "11:22:33:44:55:66".into(),
            id: None,
            managed: false,
        };
        assert_eq!(lh.version(), LighthouseVersion::V2);
    }

    #[test]
    fn test_unknown_version() {
        let lh = Lighthouse {
            name: "UnknownDevice".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: None,
            managed: false,
        };
        assert_eq!(lh.version(), LighthouseVersion::Unknown);
    }

    #[test]
    fn test_serde_roundtrip() {
        let lh = Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: None,
            managed: true,
        };
        let json = serde_json::to_string(&lh).unwrap();
        let restored: Lighthouse = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, lh);

        let v1_lh = Lighthouse {
            name: "HTC BS-AABBCCDD".into(),
            address: "11:22:33:44:55:66".into(),
            id: Some("AABBCCDD".into()),
            managed: true,
        };
        let v1_json = serde_json::to_string(&v1_lh).unwrap();
        assert!(v1_json.contains("\"id\":\"AABBCCDD\""));
        let restored_v1: Lighthouse = serde_json::from_str(&v1_json).unwrap();
        assert_eq!(restored_v1, v1_lh);
    }

    #[test]
    fn test_database_roundtrip() {
        use crate::storage;
        let lh1 = Lighthouse {
            name: "HTC BS-AABBCCDD".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: Some("AABBCCDD".into()),
            managed: true,
        };
        let lh2 = Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "11:22:33:44:55:66".into(),
            id: None,
            managed: true,
        };
        let db = storage::LighthouseDatabase {
            version: 1,
            lighthouses: vec![lh1.clone(), lh2.clone()],
        };
        let json = serde_json::to_string_pretty(&db).unwrap();
        let restored: storage::LighthouseDatabase = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.lighthouses, db.lighthouses);
    }
}
