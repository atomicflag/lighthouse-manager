#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{thread, time::Duration, time::SystemTime};

use lighthouse_manager::commands::power::{power_off, power_on};
use lighthouse_manager::storage::{load as load_settings, save as save_settings};
use openvr::{ApplicationType, init, system::event::Event};
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
        .with_ansi(false)
        .init();

    info!("Starting up...");

    // Initialise OpenVR in Background mode (no rendering, no HMD required).
    let context = unsafe { init(ApplicationType::Background) }.unwrap_or_else(|e| {
        error!("Failed to initialise OpenVR: {e}");
        std::process::exit(1);
    });

    let system = context.system().unwrap_or_else(|e| {
        error!("Failed to get OpenVR system interface: {e}");
        std::process::exit(1);
    });

    // Fire the startup hook once OpenVR is connected.
    on_steamvr_started();

    // Block in the event loop until SteamVR sends a quit event.
    run_event_loop(&system, &context);

    info!("Exited cleanly.");
}

/// Startup hook — called once, right after a successful `OpenVR` connection.
fn on_steamvr_started() {
    let Ok(rt) = Runtime::new() else {
        error!("Failed to init tokio runtime");
        return;
    };

    // Check cooldown window before turning lighthouses on.
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Ok(settings) = load_settings()
        && let Some(last_off) = settings.autostart.last_turned_off_at
    {
        let cooldown_end = last_off + settings.autostart.cooldown_secs;
        let remaining = cooldown_end.saturating_sub(now_secs);
        if now_secs < cooldown_end {
            debug!("Skipping power on — {remaining}s remaining in cooldown window");
            return;
        }
    }

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
        return;
    }

    // Update last_turned_off_at in settings after a successful shutdown.
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Ok(mut settings) = load_settings() {
        settings.autostart.last_turned_off_at = Some(now_secs);
        if let Err(e) = save_settings(&settings) {
            error!("Failed to save updated settings: {e}");
        }
    }
}

/// Main event loop. Polls `OpenVR` events at `POLL_INTERVAL` until a quit event
/// arrives, then calls the shutdown hook and returns.
fn run_event_loop(system: &openvr::System, _context: &openvr::Context) {
    info!("Entering event loop — waiting for SteamVR events...");

    loop {
        while let Some(event_info) = system.poll_next_event() {
            match event_info.event {
                // SteamVR is quitting normally.
                Event::Quit(_) => {
                    info!("Received VREvent_Quit.");
                    // Acknowledge the quit so SteamVR doesn't hang waiting for us.
                    system.acknowledge_quit_exiting();
                    on_steamvr_shutdown();
                    return;
                }

                // The driver requested a quit (e.g. crash recovery).
                Event::DriverRequestedQuit => {
                    info!("Received VREvent_DriverRequestedQuit.");
                    on_steamvr_shutdown();
                    return;
                }

                // Log other events at debug level (remove or adjust as needed).
                _ => {
                    debug!("Event: {:?}", event_info.event);
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}
