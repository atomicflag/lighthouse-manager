use anyhow::{Context, Result, anyhow, bail};
use openvr_sys as sys;
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
///
/// # Panics
///
/// Panics if `APP_KEY` contains a null byte (highly unlikely for a string
/// literal).
///
/// Steps:
///   1. Initialise `OpenVR`.
///   2. Write the manifest file (skip if it already exists).
///   3. Add the manifest to `SteamVR` (if not already installed).
///   4. Enable auto-launch.
///   5. Shut down `OpenVR`.
pub fn enable() -> Result<()> {
    let _init = OpenVrInit::try_new()?;

    let manifest_path = manifest_path()?;

    // Write manifest file if it doesn't already exist.
    write_manifest_if_missing(&manifest_path)?;

    let app_key_c = std::ffi::CString::new(APP_KEY).unwrap();

    // Check if already installed — if so, auto-launch might already be enabled.
    let already_installed = unsafe {
        let apps = get_vr_applications_table()?;
        ((*apps).IsApplicationInstalled.unwrap())(app_key_c.as_ptr().cast_mut())
    };

    if already_installed {
        info!("App already installed with SteamVR.");
    } else {
        // Add the manifest.
        let manifest_path_str = manifest_path
            .to_str()
            .ok_or_else(|| anyhow!("Manifest path contains non-UTF-8 characters"))?;
        let manifest_c = std::ffi::CString::new(manifest_path_str).unwrap();
        let app_error = unsafe {
            let apps = get_vr_applications_table()?;
            ((*apps).AddApplicationManifest.unwrap())(manifest_c.as_ptr().cast_mut(), false)
        };

        if app_error != sys::EVRApplicationError_VRApplicationError_None {
            bail!("AddApplicationManifest failed with error code {app_error}");
        }
    }

    // Enable auto-launch.
    let auto_launch_error = unsafe {
        let apps = get_vr_applications_table()?;
        ((*apps).SetApplicationAutoLaunch.unwrap())(app_key_c.as_ptr().cast_mut(), true)
    };

    if auto_launch_error != sys::EVRApplicationError_VRApplicationError_None {
        bail!("SetApplicationAutoLaunch failed with error code {auto_launch_error}");
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
///
/// # Panics
///
/// Panics if `APP_KEY` contains a null byte (highly unlikely for a string
/// literal).
///
/// Steps:
///   1. Initialise `OpenVR`.
///   2. Disable auto-launch.
///   3. Remove manifest from `SteamVR` registry (file left on disk).
///   4. Shut down `OpenVR`.
pub fn disable() -> Result<()> {
    let _init = OpenVrInit::try_new()?;

    let app_key_c = std::ffi::CString::new(APP_KEY).unwrap();

    // Disable auto-launch.
    let auto_launch_error = unsafe {
        let apps = get_vr_applications_table()?;
        ((*apps).SetApplicationAutoLaunch.unwrap())(app_key_c.as_ptr().cast_mut(), false)
    };

    if auto_launch_error != sys::EVRApplicationError_VRApplicationError_None {
        bail!("SetApplicationAutoLaunch failed with error code {auto_launch_error}");
    }

    // Remove manifest from SteamVR (does not delete the file on disk).
    let manifest_path = manifest_path()?;
    let manifest_path_str = manifest_path
        .to_str()
        .ok_or_else(|| anyhow!("Manifest path contains non-UTF-8 characters"))?;
    let manifest_c = std::ffi::CString::new(manifest_path_str).unwrap();
    let remove_error = unsafe {
        let apps = get_vr_applications_table()?;
        ((*apps).RemoveApplicationManifest.unwrap())(manifest_c.as_ptr().cast_mut())
    };

    if remove_error != sys::EVRApplicationError_VRApplicationError_None {
        bail!("RemoveApplicationManifest failed with error code {remove_error}");
    }

    info!("Autostart disabled — lighthouse-manager-ovr will no longer launch with SteamVR.");
    Ok(())
}

/// RAII guard that shuts down `OpenVR` when dropped.
struct OpenVrInit;

impl Drop for OpenVrInit {
    fn drop(&mut self) {
        unsafe { sys::VR_ShutdownInternal() };
    }
}

impl OpenVrInit {
    /// Initialise the `OpenVR` runtime. Returns None if init fails.
    fn try_new() -> Result<Self> {
        let mut vr_error = sys::EVRInitError_VRInitError_None;

        // SAFETY: VR_InitInternal is safe to call with a valid error pointer
        // and a known application-type constant.
        let token = unsafe {
            sys::VR_InitInternal(
                &raw mut vr_error,
                sys::EVRApplicationType_VRApplication_Background,
            )
        };

        if vr_error != sys::EVRInitError_VRInitError_None {
            let msg = vr_init_error_to_string(vr_error);
            return Err(anyhow!("Failed to initialise OpenVR: {msg}"));
        }

        if token == 0 {
            return Err(anyhow!("VR_InitInternal returned a null token"));
        }

        Ok(OpenVrInit)
    }
}

// Convert VRInitError to a human-readable string.
fn vr_init_error_to_string(err: sys::EVRInitError) -> String {
    let ptr = unsafe { sys::VR_GetVRInitErrorAsEnglishDescription(err) };
    if ptr.is_null() {
        return format!("Unknown error {err}");
    }
    // SAFETY: VR_GetVRInitErrorAsEnglishDescription returns a static string.
    unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

// Get the IVRApplications function table pointer.
fn get_vr_applications_table() -> Result<*mut sys::VR_IVRApplications_FnTable> {
    let iface = std::ffi::CString::new(sys::IVRApplications_Version).unwrap();
    let mut err = sys::EVRInitError_VRInitError_None;
    let ptr = unsafe { sys::VR_GetGenericInterface(iface.as_ptr(), &raw mut err) };
    if ptr == 0 {
        Err(anyhow::anyhow!(
            "Could not get IVRApplications interface (error {err})"
        ))
    } else {
        Ok(ptr as *mut sys::VR_IVRApplications_FnTable)
    }
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
