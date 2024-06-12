mod info;
mod merge;

use std::{fs::File, io::stderr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, Level};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Merge multiple spacepacket files.
    ///
    /// Contained packets must have an 8 byte CDS timecode at the start of the packet
    /// secondary header.
    Merge {
        /// Output file path.
        #[arg(short, long, default_value = "merged.dat")]
        output: PathBuf,
        /// Input spacepacket files.
        inputs: Vec<PathBuf>,
    },
    /// Show information about a spacepacket file
    Info {
        /// Input spacepacket file
        input: PathBuf,

        /// Output format
        #[arg(short, long, default_value = "json")]
        format: info::Format,

        /// Decode packet timecodes using this format.
        ///
        /// The cds timecode decoder expects timecodes in the first 8 bytes of each
        /// packets' secondary header. The eoscuc timecode decoder expects timecodes
        /// in the first 8 bytes encoded as a NASA EOS Mission timecode used for Aqua
        /// and Terra.
        #[arg(short, long, default_value = "cds")]
        timecode: info::TCFormat,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_writer(stderr)
        .with_max_level(Level::INFO)
        .init();

    info!("{}", env!("CARGO_PKG_VERSION"));

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &cli.command {
        Commands::Merge { output, inputs } => {
            let dest = File::create(output)
                .with_context(|| format!("failed to create output {output:?}"))?;
            merge::merge(inputs, &ccsds::CDSTimeDecoder, dest)
        }
        Commands::Info {
            input,
            format,
            timecode,
        } => info::info(input, format, timecode),
    }
}
