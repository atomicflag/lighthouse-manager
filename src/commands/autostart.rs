use anyhow::{Context, Result, anyhow};
use openvr::{ApplicationType, init};
use std::fs;
use std::path::PathBuf;
use tracing::info;

use crate::storage;

// Must match the one in manifest.vrmanifest exactly.
// Valve's convention is "developer.AppName" in lower-snake-case.
const APP_KEY: &str = "io.atomicflag.lighthouse-manager";

/// Enable `SteamVR` autostart for the lighthouse-manager-ovr binary.
///
/// # Errors
///
/// Returns an error if `OpenVR` initialisation fails, the manifest cannot be
/// written, or any `SteamVR` API call returns a non-success error code.
pub fn enable() -> Result<()> {
    let context = unsafe { init(ApplicationType::Utility) }?;

    let mut app = context.application()?;
    let manifest_path = manifest_path()?;

    // Write manifest file if it doesn't already exist.
    write_manifest_if_missing(&manifest_path)?;

    // Check if already installed — if so, skip.
    let already_installed = app
        .is_application_installed(APP_KEY)
        .map_err(|e| anyhow!("IsApplicationInstalled failed: {e:?}"))?;

    if already_installed {
        info!("App already installed with SteamVR.");
    } else {
        // Add the manifest.
        app.add_application_manifest(&manifest_path, false)
            .map_err(|e| anyhow!("AddApplicationManifest failed: {e:?}"))?;
    }

    info!("Autostart enabled — lighthouse-manager-ovr will launch with SteamVR.");
    Ok(())
}

/// Run the autostart command — either enable or disable based on `action`.
///
/// # Errors
///
/// Returns an error if `action` is not `'on'` or `'off'`, or if the
/// underlying enable/disable operation fails.
pub fn run(action: &str) -> Result<()> {
    match action {
        "on" => enable(),
        "off" => disable(),
        other => Err(anyhow!("Invalid action '{other}'. Expected 'on' or 'off'.")),
    }
}

/// Disable `SteamVR` autostart for the lighthouse-manager-ovr binary.
///
/// # Errors
///
/// Returns an error if `OpenVR` initialisation fails, or any `SteamVR` API call
/// returns a non-success error code.
pub fn disable() -> Result<()> {
    let context = unsafe { init(ApplicationType::Utility) }?;

    let mut app = context.application()?;
    let manifest_path = manifest_path()?;

    // Remove manifest from SteamVR (does not delete the file on disk).
    app.remove_application_manifest(&manifest_path)
        .map_err(|e| anyhow!("RemoveApplicationManifest failed: {e:?}"))?;

    info!("Autostart disabled — lighthouse-manager-ovr will no longer launch with SteamVR.");
    Ok(())
}

// Path to manifest.vrmanifest inside the config local directory.
fn manifest_path() -> Result<PathBuf> {
    let dir = storage::config_local_dir().context("Could not determine config directory")?;
    Ok(dir.join("manifest.vrmanifest"))
}

// Write the vrmanifest to disk if it doesn't already exist.
// Uses the -ovr binary name, derived from the CLI executable's location.
fn write_manifest_if_missing(path: &PathBuf) -> Result<()> {
    if path.exists() {
        return Ok(()); // Already written on a previous run.
    }

    let ovr_exe_path = ovr_binary_path()?;
    let exe_name = ovr_exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("Could not determine -ovr executable file name"))?;

    // The binary path in the manifest must match the actual executable name.
    let manifest = format!(
        r#"{{
  "source": "builtin",
  "applications": [
    {{
      "app_key": "{APP_KEY}",
      "launch_type": "binary",
      "binary_path_windows": "{exe_name}",
      "binary_path_linux":   "{exe_name}",
      "binary_path_osx":     "{exe_name}",
      "is_dashboard_overlay": true,
      "strings": {{
        "en_us": {{
          "name": "Lighthouse Manager",
          "description": "Tool that lets you discover, power on/off, and identify SteamVR Lighthouse base stations wirelessly via Bluetooth Low Energy."
        }}
      }}
    }}
  ]
}}
"#
    );

    fs::write(path, manifest).context("Could not write manifest")?;
    info!("Wrote manifest to: {}", path.display());
    Ok(())
}

// Construct the path to the -ovr binary by deriving it from the CLI exe.
// Both executables are expected to live in the same directory.
fn ovr_binary_path() -> Result<PathBuf> {
    let cli_exe = std::env::current_exe().context("Could not determine current executable path")?;

    let parent = cli_exe.parent().ok_or_else(|| {
        anyhow::anyhow!("Could not determine parent directory of current executable")
    })?;

    let cli_stem = cli_exe
        .file_stem()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("CLI exe has no file stem"))?;

    // Append "-ovr" to the CLI binary name (e.g. "lighthouse-manager" → "lighthouse-manager-ovr").
    let ovr_name = format!(
        "{cli_stem}-ovr{}",
        cli_exe
            .extension()
            .map(|e| format!(".{}", e.to_str().unwrap_or("")))
            .unwrap_or_default()
    );

    Ok(parent.join(ovr_name))
}
