use clap::Parser;
use lighthouse_manager::commands;

/// Enable or disable `SteamVR` autostart for the companion binary.
#[derive(Parser)]
pub struct AutostartArgs {
    /// "on" to enable autostart, "off" to disable it.
    pub action: String,
}

pub fn run(args: &AutostartArgs) -> anyhow::Result<()> {
    match args.action.as_str() {
        "on" => commands::autostart::enable(),
        "off" => commands::autostart::disable(),
        other => Err(anyhow::anyhow!(
            "Invalid action '{other}'. Expected 'on' or 'off'."
        )),
    }
}
