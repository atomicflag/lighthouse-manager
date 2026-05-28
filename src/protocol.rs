/// Protocol helpers for building Lighthouse power control commands.
use crate::lighthouse::{Lighthouse, LighthouseVersion};

// GATT Service UUIDs
pub const V1_POWER_CHARACTERISTIC: &str = "0000cb01-0000-1000-8000-00805f9b34fb";
pub const V2_POWER_CHARACTERISTIC: &str = "00001525-1212-efde-1523-785feabcd124";
pub const V2_IDENTIFY_CHARACTERISTIC: &str = "00008421-1212-efde-1523-785feabcd124";

/// Build the power-on command bytes for a V1 lighthouse.
/// Format: [0x12, 0x00, 0x00, 0x00] + reversed ID bytes (4) + [0x00; 12] = 20 bytes total.
pub fn build_v1_power_on(id: &str) -> Result<Vec<u8>, String> {
    validate_v1_id(id)?;
    let id_bytes = parse_v1_id_bytes(id);
    let mut cmd = vec![0x12, 0x00, 0x00, 0x00];
    let mut rev_id = id_bytes.clone();
    rev_id.reverse();
    cmd.extend_from_slice(&rev_id); // reversed
    cmd.resize(20, 0x00); // pad to 20 bytes
    Ok(cmd)
}

/// Build the sleep command bytes for a V1 lighthouse.
/// Format: [0x12, 0x02, 0x00, 0x01] + reversed ID bytes (4) + [0x00; 12] = 20 bytes total.
pub fn build_v1_sleep(id: &str) -> Result<Vec<u8>, String> {
    validate_v1_id(id)?;
    let id_bytes = parse_v1_id_bytes(id);
    let mut cmd = vec![0x12, 0x02, 0x00, 0x01];
    let mut rev_id = id_bytes.clone();
    rev_id.reverse();
    cmd.extend_from_slice(&rev_id); // reversed
    cmd.resize(20, 0x00); // pad to 20 bytes
    Ok(cmd)
}

/// Build the power-on command for a V2 lighthouse.
pub fn build_v2_power_on() -> Vec<u8> {
    vec![0x01]
}

/// Build the sleep command for a V2 lighthouse.
pub fn build_v2_sleep() -> Vec<u8> {
    vec![0x00]
}

/// Build the identify command for a V2 lighthouse.
pub fn build_v2_identify() -> Vec<u8> {
    vec![0x01]
}

/// Validate that an ID is exactly 8 hex characters.
fn validate_v1_id(id: &str) -> Result<(), String> {
    if id.len() != 8 {
        return Err(format!("Invalid V1 ID length: {} (expected 8 chars)", id));
    }
    if !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("V1 ID contains non-hex characters: {}", id));
    }
    Ok(())
}

/// Parse an 8-char hex ID string into 4 bytes.
/// E.g., "AABBCCDD" → [0xAA, 0xBB, 0xCC, 0xDD]
fn parse_v1_id_bytes(id: &str) -> Vec<u8> {
    (0..id.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&id[i..i + 2], 16).unwrap_or(0))
        .collect()
}

/// Build the power control command bytes for a lighthouse.
pub fn build_power_command(lh: &Lighthouse) -> Result<Vec<u8>, String> {
    match lh.version() {
        LighthouseVersion::V1 => {
            let id = lh
                .id
                .as_ref()
                .ok_or_else(|| "V1 lighthouse missing ID for power command".to_string())?;
            build_v1_power_on(id)
        }
        LighthouseVersion::V2 => Ok(build_v2_power_on()),
        LighthouseVersion::Unknown => Err("Cannot build power command for unknown version".into()),
    }
}

/// Build the sleep control command bytes for a lighthouse.
pub fn build_sleep_command(lh: &Lighthouse) -> Result<Vec<u8>, String> {
    match lh.version() {
        LighthouseVersion::V1 => {
            let id = lh
                .id
                .as_ref()
                .ok_or_else(|| "V1 lighthouse missing ID for sleep command".to_string())?;
            build_v1_sleep(id)
        }
        LighthouseVersion::V2 => Ok(build_v2_sleep()),
        LighthouseVersion::Unknown => Err("Cannot build sleep command for unknown version".into()),
    }
}

/// Build the identify command bytes for a lighthouse (V2 only).
pub fn build_identify_command(lh: &Lighthouse) -> Result<Vec<u8>, String> {
    match lh.version() {
        LighthouseVersion::V2 => Ok(build_v2_identify()),
        LighthouseVersion::V1 => Err("Identify is not supported on V1 lighthouses".into()),
        LighthouseVersion::Unknown => {
            Err("Cannot build identify command for unknown version".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v1_power_on_command() {
        let cmd = build_v1_power_on("AABBCCDD").unwrap();
        assert_eq!(cmd.len(), 20);
        assert_eq!(cmd[0], 0x12);
        assert_eq!(cmd[1], 0x00);
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x00);
        // ID bytes reversed: AABBCCDD → DD CC BB AA
        assert_eq!(&cmd[4..8], &[0xDD, 0xCC, 0xBB, 0xAA]);
        // Remaining are zeros
        assert_eq!(&cmd[8..20], &[0u8; 12]);
    }

    #[test]
    fn test_v1_sleep_command() {
        let cmd = build_v1_sleep("AABBCCDD").unwrap();
        assert_eq!(cmd.len(), 20);
        assert_eq!(cmd[0], 0x12);
        assert_eq!(cmd[1], 0x02);
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x01);
        // ID bytes reversed
        assert_eq!(&cmd[4..8], &[0xDD, 0xCC, 0xBB, 0xAA]);
    }

    #[test]
    fn test_v2_commands() {
        assert_eq!(build_v2_power_on(), vec![0x01]);
        assert_eq!(build_v2_sleep(), vec![0x00]);
        assert_eq!(build_v2_identify(), vec![0x01]);
    }

    #[test]
    fn test_validate_invalid_id() {
        assert!(build_v1_power_on("12345").is_err()); // too short
        assert!(build_v1_power_on("GGHHIIJJ").is_err()); // non-hex
        assert!(build_v1_power_on("AABBCCDD11").is_err()); // too long
    }

    #[test]
    fn test_parse_v1_id_bytes() {
        let bytes = parse_v1_id_bytes("AABBCCDD");
        assert_eq!(bytes, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_build_power_command_v1() {
        let lh = Lighthouse {
            name: "HTC BS-AABBCCDD".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: Some("AABBCCDD".into()),
            managed: true,
        };
        let cmd = build_power_command(&lh).unwrap();
        assert_eq!(cmd.len(), 20);
        // Should match v1_power_on
        assert_eq!(cmd, build_v1_power_on("AABBCCDD").unwrap());
    }

    #[test]
    fn test_build_power_command_v2() {
        let lh = Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "11:22:33:44:55:66".into(),
            id: None,
            managed: true,
        };
        let cmd = build_power_command(&lh).unwrap();
        assert_eq!(cmd, vec![0x01]);
    }

    #[test]
    fn test_build_sleep_command_v2() {
        let lh = Lighthouse {
            name: "LHB-0A1B2C3D".into(),
            address: "11:22:33:44:55:66".into(),
            id: None,
            managed: true,
        };
        let cmd = build_sleep_command(&lh).unwrap();
        assert_eq!(cmd, vec![0x00]);
    }

    #[test]
    fn test_identify_v1_fails() {
        let lh = Lighthouse {
            name: "HTC BS-AABBCCDD".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: Some("AABBCCDD".into()),
            managed: true,
        };
        assert!(build_identify_command(&lh).is_err());
    }

    #[test]
    fn test_build_power_command_missing_id() {
        let lh = Lighthouse {
            name: "HTC BS-AABBCCDD".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            id: None,
            managed: true,
        };
        assert!(build_power_command(&lh).is_err());
    }
}
