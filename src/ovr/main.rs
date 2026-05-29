// ---------------------------------------------------------------------------
// Safety note: the entire OpenVR C API is `unsafe`.  We isolate all FFI calls
// inside dedicated functions so that `main` stays readable.
// ---------------------------------------------------------------------------

use std::{env, fs, path::PathBuf, thread, time::Duration};

use openvr_sys as sys;

// ---------------------------------------------------------------------------
// Application key — must match the one in manifest.vrmanifest exactly.
// Valve's convention is "developer.AppName" in lower-snake-case.
// ---------------------------------------------------------------------------
const APP_KEY: &str = "mydev.steamvr_companion";

// ---------------------------------------------------------------------------
// How long to sleep between event-poll iterations (keeps CPU usage negligible).
// ---------------------------------------------------------------------------
const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn main() {
    println!("[steamvr-companion] Starting up...");

    // ------------------------------------------------------------------
    // 1. Initialise OpenVR in Background mode.
    //    VRApplication_Background means we do NOT render anything and we
    //    do NOT require an HMD to be connected.  This is the correct mode
    //    for utility/companion processes.
    // ------------------------------------------------------------------
    let vr_system = match init_openvr() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[steamvr-companion] Failed to initialise OpenVR: {e}");
            eprintln!(
                "Make sure SteamVR is running before launching this program for the first time."
            );
            std::process::exit(1);
        }
    };

    // ------------------------------------------------------------------
    // 2. Register our application manifest and enable auto-launch.
    //    This only needs to happen once; subsequent launches skip it.
    // ------------------------------------------------------------------
    match register_manifest() {
        Ok(already) if already => {
            println!("[steamvr-companion] Already registered with SteamVR.");
        }
        Ok(_) => {
            println!(
                "[steamvr-companion] Successfully registered with SteamVR — auto-launch enabled."
            );
        }
        Err(e) => {
            // Non-fatal: the app still works this session, it just won't
            // auto-launch next time.
            eprintln!("[steamvr-companion] Warning: could not register manifest: {e}");
        }
    }

    // ------------------------------------------------------------------
    // 3. SteamVR is up and we are connected — fire the startup hook.
    // ------------------------------------------------------------------
    on_steamvr_started();

    // ------------------------------------------------------------------
    // 4. Event loop — block until SteamVR sends a quit event.
    // ------------------------------------------------------------------
    run_event_loop(vr_system);

    // ------------------------------------------------------------------
    // 5. Shut down OpenVR cleanly before exiting.
    // ------------------------------------------------------------------
    unsafe { sys::VR_ShutdownInternal() };
    println!("[steamvr-companion] Exited cleanly.");
}

// ---------------------------------------------------------------------------
// Startup hook — called once, right after a successful OpenVR connection.
// Replace the println! with whatever you need to do on SteamVR launch.
// ---------------------------------------------------------------------------
fn on_steamvr_started() {
    println!("[steamvr-companion] >>> SteamVR has started! (on_steamvr_started hook)");
    // TODO: add your startup logic here.
    // Examples: spawn background threads, connect to hardware, load config …
}

// ---------------------------------------------------------------------------
// Shutdown hook — called once, when SteamVR signals it is about to quit.
// Replace the println! with your own teardown logic.
// ---------------------------------------------------------------------------
fn on_steamvr_shutdown() {
    println!("[steamvr-companion] >>> SteamVR is shutting down! (on_steamvr_shutdown hook)");
    // TODO: add your shutdown logic here.
    // Examples: save state, disconnect hardware, flush logs …
}

// ---------------------------------------------------------------------------
// Initialise the OpenVR runtime.
// Returns a raw pointer to IVRSystem (only used to call PollNextEvent on it).
// ---------------------------------------------------------------------------
fn init_openvr() -> Result<*mut sys::VR_IVRSystem_FnTable, String> {
    let mut vr_error = sys::EVRInitError_VRInitError_None;

    // SAFETY: VR_InitInternal is safe to call as long as we pass a valid
    // pointer for the error output and a known application-type constant.
    let token = unsafe {
        sys::VR_InitInternal(
            &raw mut vr_error,
            sys::EVRApplicationType_VRApplication_Background,
        )
    };

    if vr_error != sys::EVRInitError_VRInitError_None {
        let msg = vr_init_error_to_string(vr_error);
        return Err(msg);
    }

    if token == 0 {
        return Err("VR_InitInternal returned a null token".to_string());
    }

    // Obtain the IVRSystem function table.
    // The interface version string is defined in the openvr_sys crate.
    let iface_version = std::ffi::CString::new(sys::IVRSystem_Version).expect("CString");
    let mut err2 = sys::EVRInitError_VRInitError_None;

    // SAFETY: VR_GetGenericInterface is safe given a valid token and version.
    let ptr = unsafe { sys::VR_GetGenericInterface(iface_version.as_ptr(), &raw mut err2) };

    if ptr == 0 {
        return Err(format!(
            "VR_GetGenericInterface returned null (error {err2})"
        ));
    }

    Ok(ptr as *mut sys::VR_IVRSystem_FnTable)
}

// ---------------------------------------------------------------------------
// Register our .vrmanifest with SteamVR and enable auto-launch.
// Returns Ok(true)  if we were already registered.
// Returns Ok(false) if we just registered successfully.
// Returns Err(…)    on failure.
// ---------------------------------------------------------------------------
fn register_manifest() -> Result<bool, String> {
    // Build the absolute path to manifest.vrmanifest, sitting next to the exe.
    let manifest_path = manifest_path()?;
    let manifest_path_str = manifest_path
        .to_str()
        .ok_or("Manifest path contains non-UTF-8 characters")?;

    // Write the manifest file if it doesn't exist yet.
    write_manifest_if_missing(&manifest_path)?;

    let app_key_c = std::ffi::CString::new(APP_KEY).unwrap();

    // Check if already installed.
    let already_installed = unsafe {
        // SAFETY: VR_GetGenericInterface gives us the IVRApplications vtable.
        let apps = get_vr_applications_table()?;
        ((*apps).IsApplicationInstalled.unwrap())(app_key_c.as_ptr().cast_mut())
    };

    if already_installed {
        return Ok(true);
    }

    // Add the manifest.
    let manifest_c = std::ffi::CString::new(manifest_path_str).unwrap();
    let app_error = unsafe {
        let apps = get_vr_applications_table()?;
        ((*apps).AddApplicationManifest.unwrap())(manifest_c.as_ptr().cast_mut(), false)
    };

    if app_error != sys::EVRApplicationError_VRApplicationError_None {
        return Err(format!(
            "AddApplicationManifest failed with error code {app_error}"
        ));
    }

    // Enable auto-launch.
    let auto_launch_error = unsafe {
        let apps = get_vr_applications_table()?;
        ((*apps).SetApplicationAutoLaunch.unwrap())(app_key_c.as_ptr().cast_mut(), true)
    };

    if auto_launch_error != sys::EVRApplicationError_VRApplicationError_None {
        return Err(format!(
            "SetApplicationAutoLaunch failed with error code {auto_launch_error}"
        ));
    }

    Ok(false)
}

// ---------------------------------------------------------------------------
// Main event loop.  Polls OpenVR events at POLL_INTERVAL until a quit event
// arrives, then calls the shutdown hook and returns.
// ---------------------------------------------------------------------------
fn run_event_loop(vr_system: *mut sys::VR_IVRSystem_FnTable) {
    println!("[steamvr-companion] Entering event loop — waiting for SteamVR events...");

    loop {
        // Poll all pending events before sleeping.
        loop {
            let mut event: sys::VREvent_t = unsafe { std::mem::zeroed() };
            let has_event = unsafe {
                ((*vr_system).PollNextEvent.unwrap())(
                    &raw mut event,
                    std::mem::size_of::<sys::VREvent_t>() as u32,
                )
            };

            if !has_event {
                break; // No more events queued right now.
            }

            match event.eventType {
                // SteamVR is quitting normally.
                t if t == sys::EVREventType_VREvent_Quit => {
                    println!("[steamvr-companion] Received VREvent_Quit.");
                    on_steamvr_shutdown();
                    // Acknowledge the quit so SteamVR doesn't hang waiting for us.
                    unsafe {
                        ((*vr_system).AcknowledgeQuit_Exiting.unwrap())();
                    }
                    return;
                }

                // The driver requested a quit (e.g. crash recovery).
                t if t == sys::EVREventType_VREvent_DriverRequestedQuit => {
                    println!("[steamvr-companion] Received VREvent_DriverRequestedQuit.");
                    on_steamvr_shutdown();
                    return;
                }

                // Log other events at trace level (remove or adjust as needed).
                other => {
                    println!("[steamvr-companion] Event: type={other}");
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// Helper: get a pointer to the IVRApplications function table.
// ---------------------------------------------------------------------------
fn get_vr_applications_table() -> Result<*mut sys::VR_IVRApplications_FnTable, String> {
    let iface = std::ffi::CString::new(sys::IVRApplications_Version).unwrap();
    let mut err = sys::EVRInitError_VRInitError_None;
    let ptr = unsafe { sys::VR_GetGenericInterface(iface.as_ptr(), &raw mut err) };
    if ptr == 0 {
        Err(format!(
            "Could not get IVRApplications interface (error {err})"
        ))
    } else {
        Ok(ptr as *mut sys::VR_IVRApplications_FnTable)
    }
}

// ---------------------------------------------------------------------------
// Helper: convert a VRInitError to a human-readable string.
// ---------------------------------------------------------------------------
fn vr_init_error_to_string(err: sys::EVRInitError) -> String {
    // SAFETY: VR_GetVRInitErrorAsEnglishDescription returns a static string.
    let ptr = unsafe { sys::VR_GetVRInitErrorAsEnglishDescription(err) };
    if ptr.is_null() {
        return format!("Unknown error {err}");
    }
    unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

// ---------------------------------------------------------------------------
// Helper: path to manifest.vrmanifest (next to the binary).
// ---------------------------------------------------------------------------
fn manifest_path() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or("Could not determine executable directory")?;
    Ok(dir.join("manifest.vrmanifest"))
}

// ---------------------------------------------------------------------------
// Write the vrmanifest to disk if it doesn't exist yet.
//
// The manifest tells SteamVR:
//   • The unique app_key (must match APP_KEY above).
//   • launch_type = "binary" → SteamVR launches our exe directly.
//   • The binary path (relative to the manifest file).
//   • is_dashboard_overlay = true → shows up in the Startup/Shutdown list.
// ---------------------------------------------------------------------------
fn write_manifest_if_missing(path: &PathBuf) -> Result<(), String> {
    if path.exists() {
        return Ok(()); // Already written on a previous run.
    }

    // The binary path in the manifest must match the actual executable name.
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let exe_name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Could not determine executable file name")?;

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
          "name": "SteamVR Companion",
          "description": "A minimal companion app with startup/shutdown hooks."
        }}
      }}
    }}
  ]
}}
"#
    );

    fs::write(path, manifest).map_err(|e| format!("Could not write manifest: {e}"))?;
    println!("[steamvr-companion] Wrote manifest to: {}", path.display());
    Ok(())
}
