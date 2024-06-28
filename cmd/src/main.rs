mod info;
mod merge;

use std::path::PathBuf;
use std::{fs::File, io::stderr};

use anyhow::{Context, Result};
use ccsds::Apid;
use clap::{Parser, Subcommand};
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

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
    ///
    /// The merge process will reorder packets by time and APID. To write the merged
    /// packets in a specific order see --apid-order.
    Merge {
        /// Manually set the APID order the merged packets for the same time are written.
        ///
        /// Any unspecified APIDs will be sorted by their numerical APID value. This will
        /// only affect packets with the same time and different APIDs.
        ///
        /// For example, given APIDs 1, 2, 3, 4 and a desired output order of 4, 2, 1, 3
        /// you could specify --apid-order=4,2,1. Note, 1 must be specified to give
        /// a mapping of 4:0, 2:1, 1:2, 3:3, otherwise the mapping would be 4:0, 2:1, 1:1,
        /// 3:3 where 2 and 1 both map to sort index 1 which could lead to ambiguios ordering.
        #[arg(short = 'O', long, value_delimiter = ',')]
        apid_order: Option<Vec<Apid>>,
        /// Alias for --apid-order-826,821
        #[arg(long, hide = true, default_value = "false")]
        viirs: bool,
        /// Output file path.
        #[arg(short, long, default_value = "merged.dat", value_parser = must_not_exist)]
        output: PathBuf,
        /// Input spacepacket files.
        inputs: Vec<PathBuf>,
    },
    /// Show information about a spacepacket file
    Info {
        /// Input spacepacket file
        input: PathBuf,

        /// Output format
        #[arg(short, long, default_value = "text")]
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

fn must_not_exist(s: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(s);
    if p.exists() {
        Err(format!("{s} already exists"))
    } else {
        Ok(p)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(stderr)
        .with_ansi(false)
        .without_time()
        .with_env_filter(
            EnvFilter::try_from_env("CCSDS_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    debug!(
        "{} {} ({})",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("GIT_SHA")
    );

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &cli.command {
        Commands::Merge {
            output,
            inputs,
            apid_order,
            viirs,
        } => {
            info!("merging {:?}", inputs);
            info!("to {output:?}");
            let apid_order = if *viirs {
                Some(vec![826, 821])
            } else {
                apid_order.as_deref().map_or(None, |s| Some(s.to_vec()))
            };
            let dest = File::create(output)
                .with_context(|| format!("failed to create output {output:?}"))?;
            merge::merge(inputs, &ccsds::CDSTimeDecoder, dest, apid_order)
        }
        Commands::Info {
            input,
            format,
            timecode,
        } => info::info(input, format, timecode),
    }
}
