use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone)]
enum Timecode {
    CDS,
    EOSCUC,
}

impl std::fmt::Display for Timecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Timecode::CDS => "cds",
                Timecode::EOSCUC => "eoscuc",
            }
        )
    }
}

impl std::str::FromStr for Timecode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cds" => Ok(Timecode::CDS),
            "eoscuc" => Ok(Timecode::EOSCUC),
            _ => Err(format!("Unknown timecode format: {s}")),
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Merge multiple spacepacket files.
    Merge {
        /// Input spacepacket files.
        #[arg(short, long, num_args=1.., value_delimiter=',', required=true)]
        inputs: Vec<PathBuf>,

        /// Packet timecode parser.
        #[arg(short, long, default_value_t=Timecode::CDS)]
        timecode: Timecode,
    },
}

fn main() {
    let cli = Cli::parse();

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &cli.command {
        Commands::Merge { inputs, timecode } => {
            dbg!(inputs);
            dbg!(timecode);
        }
    }
}
