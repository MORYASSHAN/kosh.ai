//! The `kosh` command.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "kosh",
    version,
    about = "Kosh Doctor: an honest device health check. Read-only."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Scan this computer.
    Scan {
        /// Print JSON instead of a summary.
        #[arg(long)]
        json: bool,
        /// Seconds to sample memory use.
        #[arg(long, default_value_t = 30)]
        sample_secs: u32,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("KOSH_LOG"))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Scan {
            json: _,
            sample_secs,
        }) => {
            let ctx = kosh_collectors::Ctx {
                sample_secs,
                ..Default::default()
            };
            let snap = kosh_collectors::scan(&ctx, None);
            println!("{}", serde_json::to_string_pretty(&snap)?);
        }
        None => {
            println!("kosh {}", env!("CARGO_PKG_VERSION"));
        }
    }
    Ok(())
}
