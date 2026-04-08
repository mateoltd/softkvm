use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "softkvm", about = "softkvm command line interface")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// scan for monitors with DDC/CI support
    Scan {
        /// output as JSON for machine consumption
        #[arg(long)]
        json: bool,
    },
    /// manually switch a monitor's input source
    Switch {
        /// monitor ID (from scan output)
        monitor_id: String,
        /// target input source (e.g., HDMI1, DisplayPort1, 0x0f)
        input: String,
    },
    /// validate a configuration file
    #[command(name = "validate")]
    ValidateConfig {
        /// path to config file
        #[arg(short, long, default_value = "softkvm.toml")]
        config: String,
    },
    /// identify a monitor by briefly blanking it
    Identify {
        /// monitor ID (from scan output)
        monitor_id: String,
    },
    /// show current system status
    Status,
    /// check for updates and apply them
    Update,
    /// run interactive setup
    Setup,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { json } => commands::scan::run(json).await,
        Commands::Switch { monitor_id, input } => commands::switch::run(&monitor_id, &input).await,
        Commands::Identify { monitor_id } => commands::identify::run(&monitor_id).await,
        Commands::ValidateConfig { config } => commands::validate::run(&config),
        Commands::Status => commands::status::run().await,
        Commands::Update => commands::update::run().await,
        Commands::Setup => commands::setup::run().await,
    }
}
