// ---------------------------------------------------------------------------
// Safety note: the entire OpenVR C API is `unsafe`.  We isolate all FFI calls
// inside dedicated functions so that `main` stays readable.
// ---------------------------------------------------------------------------

use std::{thread, time::Duration};

use openvr_sys as sys;
use tracing::{debug, error, info};

// ---------------------------------------------------------------------------
// How long to sleep between event-poll iterations (keeps CPU usage negligible).
// ---------------------------------------------------------------------------
const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("[steamvr-companion] Starting up...");

    // ------------------------------------------------------------------
    // 1. Initialise OpenVR in Background mode.
    //    VRApplication_Background means we do NOT render anything and we
    //    do NOT require an HMD to be connected.  This is the correct mode
    //    for utility/companion processes.
    // ------------------------------------------------------------------
    let vr_system = match init_openvr() {
        Ok(s) => s,
        Err(e) => {
            error!("[steamvr-companion] Failed to initialise OpenVR: {}", e);
            error!(
                "Make sure SteamVR is running before launching this program for the first time."
            );
            std::process::exit(1);
        }
    };

    // ------------------------------------------------------------------
    // 2. SteamVR is up and we are connected — fire the startup hook.
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
    info!("[steamvr-companion] Exited cleanly.");
}

// ---------------------------------------------------------------------------
// Startup hook — called once, right after a successful OpenVR connection.
// Replace the tracing call with whatever you need to do on SteamVR launch.
// ---------------------------------------------------------------------------
fn on_steamvr_started() {
    info!("[steamvr-companion] >>> SteamVR has started! (on_steamvr_started hook)");
    // TODO: add your startup logic here.
    // Examples: spawn background threads, connect to hardware, load config …
}

// ---------------------------------------------------------------------------
// Shutdown hook — called once, when SteamVR signals it is about to quit.
// Replace the tracing call with your own teardown logic.
// ---------------------------------------------------------------------------
fn on_steamvr_shutdown() {
    info!("[steamvr-companion] >>> SteamVR is shutting down! (on_steamvr_shutdown hook)");
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
// Main event loop.  Polls OpenVR events at POLL_INTERVAL until a quit event
// arrives, then calls the shutdown hook and returns.
// ---------------------------------------------------------------------------
fn run_event_loop(vr_system: *mut sys::VR_IVRSystem_FnTable) {
    info!("[steamvr-companion] Entering event loop — waiting for SteamVR events...");

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
                    info!("[steamvr-companion] Received VREvent_Quit.");
                    on_steamvr_shutdown();
                    // Acknowledge the quit so SteamVR doesn't hang waiting for us.
                    unsafe {
                        ((*vr_system).AcknowledgeQuit_Exiting.unwrap())();
                    }
                    return;
                }

                // The driver requested a quit (e.g. crash recovery).
                t if t == sys::EVREventType_VREvent_DriverRequestedQuit => {
                    info!("[steamvr-companion] Received VREvent_DriverRequestedQuit.");
                    on_steamvr_shutdown();
                    return;
                }

                // Log other events at debug level (remove or adjust as needed).
                other => {
                    debug!("[steamvr-companion] Event: type={other}");
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
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
