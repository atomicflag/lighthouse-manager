// The entire OpenVR C API is `unsafe` — we isolate all FFI calls inside
// dedicated functions so that `main` stays readable.

use std::{thread, time::Duration};

use anyhow::{Context, Result, bail};
use lighthouse_manager::commands::power::{power_off, power_on};
use openvr_sys as sys;
use tokio::runtime::Runtime;
use tracing::{debug, error, info};

// How long to sleep between event-poll iterations (keeps CPU usage negligible).
const POLL_INTERVAL: Duration = Duration::from_secs(5);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .event_format(tracing_subscriber::fmt::format().with_target(false))
        .init();

    info!("Starting up...");

    // Initialise OpenVR in Background mode (no rendering, no HMD required).
    let vr_system = init_openvr().unwrap_or_else(|e| {
        error!("Failed to initialise OpenVR: {e}");
        std::process::exit(1);
    });

    // Fire the startup hook once OpenVR is connected.
    on_steamvr_started();

    // Block in the event loop until SteamVR sends a quit event.
    run_event_loop(vr_system);

    // Shut down OpenVR cleanly before exiting.
    unsafe { sys::VR_ShutdownInternal() };
    info!("Exited cleanly.");
}

/// Startup hook — called once, right after a successful `OpenVR` connection.
fn on_steamvr_started() {
    let Ok(rt) = Runtime::new() else {
        error!("Failed to init tokio runtime");
        return;
    };

    if let Err(e) = rt.block_on(power_on()) {
        error!("Failed to power on lighthouses: {e}");
    }
}

/// Shutdown hook — called once, when `SteamVR` signals it is about to quit.
fn on_steamvr_shutdown() {
    let Ok(rt) = Runtime::new() else {
        error!("Failed to init tokio runtime");
        return;
    };

    if let Err(e) = rt.block_on(power_off()) {
        error!("Failed to power off lighthouses: {e}");
    }
}

/// Initialise the `OpenVR` runtime.
/// Returns a raw pointer to `IVRSystem` (only used to call `PollNextEvent` on it).
fn init_openvr() -> Result<*mut sys::VR_IVRSystem_FnTable> {
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
        bail!(
            "VR_InitInternal failed: {}",
            vr_init_error_to_string(vr_error)
        );
    }

    if token == 0 {
        bail!("VR_InitInternal returned a null token");
    }

    // Obtain the IVRSystem function table.
    // The interface version string is defined in the openvr_sys crate.
    let iface_version = std::ffi::CString::new(sys::IVRSystem_Version).context("CString")?;
    let mut err2 = sys::EVRInitError_VRInitError_None;

    // SAFETY: VR_GetGenericInterface is safe given a valid token and version.
    let ptr = unsafe { sys::VR_GetGenericInterface(iface_version.as_ptr(), &raw mut err2) };

    if ptr == 0 {
        bail!("VR_GetGenericInterface returned null (error {err2})");
    }

    Ok(ptr as *mut sys::VR_IVRSystem_FnTable)
}

/// Main event loop. Polls `OpenVR` events at `POLL_INTERVAL` until a quit event
/// arrives, then calls the shutdown hook and returns.
fn run_event_loop(vr_system: *mut sys::VR_IVRSystem_FnTable) {
    info!("Entering event loop — waiting for SteamVR events...");

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
                    info!("Received VREvent_Quit.");
                    on_steamvr_shutdown();
                    // Acknowledge the quit so SteamVR doesn't hang waiting for us.
                    unsafe {
                        ((*vr_system).AcknowledgeQuit_Exiting.unwrap())();
                    }
                    return;
                }

                // The driver requested a quit (e.g. crash recovery).
                t if t == sys::EVREventType_VREvent_DriverRequestedQuit => {
                    info!("Received VREvent_DriverRequestedQuit.");
                    on_steamvr_shutdown();
                    return;
                }

                // Log other events at debug level (remove or adjust as needed).
                other => {
                    debug!("Event: type={other}");
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

/// Helper: convert a `VRInitError` to a human-readable string.
fn vr_init_error_to_string(err: sys::EVRInitError) -> String {
    // SAFETY: VR_GetVRInitErrorAsEnglishDescription returns a static string.
    let ptr = unsafe { sys::VR_GetVRInitErrorAsEnglishDescription(err) };
    if ptr.is_null() {
        return format!("Unknown error {err}");
    }
    unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}
