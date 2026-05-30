use anyhow::Result;
use clap::{Parser, Subcommand, builder::BoolishValueParser};
use lighthouse_manager::{bluetooth, commands};
use tracing::{error, info};

mod autostart;

#[derive(Parser)]
#[command(name = "lighthouse-manager")]
#[command(version)]
#[command(about = "Control SteamVR Lighthouse base stations (V1/V2) via Bluetooth LE.", long_about = None)]
struct Cli {
    /// Verbosity level (use -v for debug, -vv for trace). Default: info level.
    #[arg(short, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan for nearby Lighthouse base stations and save them to the database.
    /// Newly discovered units are saved automatically (unmanaged by default).
    Discover {
        /// Scan duration in seconds. Default: 10.
        #[arg(short, long, default_value_t = 10)]
        duration: u64,
    },

    /// List all known lighthouses or filter by managed status.
    List {
        /// Only show managed lighthouses.
        #[arg(long)]
        managed: bool,

        /// Output in JSON format.
        #[arg(long)]
        json: bool,
    },

    /// Power on all managed lighthouses in parallel.
    PowerOn,

    /// Sleep (power off) all managed lighthouses in parallel.
    /// V1 units only support sleep; V2 also supports standby.
    PowerOff,

    /// Identify a specific V2 lighthouse by database index (causes LED flash).
    #[command(alias = "blink")]
    Identify {
        /// Zero-based index of the lighthouse in the database.
        index: usize,
    },

    /// Set one or all lighthouses as managed or unmanaged.
    SetManaged {
        /// Index of a single lighthouse, or "all" to set every entry.
        target: String,
        /// true = manage this lighthouse, false = ignore it.
        #[arg(action = clap::ArgAction::Set, value_parser = BoolishValueParser::new())]
        managed: bool,
    },

    /// Enable or disable `SteamVR` autostart for the companion binary.
    Autostart {
        /// "on" to enable autostart, "off" to disable it.
        action: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing subscriber based on verbosity level
    let filter = match cli.verbose {
        0 => "lighthouse_manager=info",
        1 => "lighthouse_manager=debug",
        _ => "lighthouse_manager=trace",
    };

    let builder = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .event_format(tracing_subscriber::fmt::format().with_target(false));

    builder.init();

    // Verify Bluetooth adapter is available (early check)
    match bluetooth::get_adapter().await {
        Ok(_) => info!("Bluetooth adapter ready."),
        Err(e) => {
            error!("{}", e);
            return Err(e);
        }
    }

    let result = match cli.command {
        Command::Discover { duration } => commands::discover::run(duration).await,
        Command::List { managed, json } => commands::list::run(managed, json),
        Command::PowerOn => commands::power::power_on().await,
        Command::PowerOff => commands::power::power_off().await,
        Command::Identify { index } => commands::identify::run(index).await,
        Command::SetManaged { target, managed } => commands::set_managed::run(&target, managed),
        Command::Autostart { action } => {
            let args = autostart::AutostartArgs { action };
            autostart::run(&args)
        }
    };

    if let Err(e) = &result {
        error!("{}", e);
    }

    result
}
