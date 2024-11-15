mod filter;
mod info;
mod merge;
mod spacecraft;

use std::path::PathBuf;
use std::str::FromStr;
use std::{fs::File, io::stderr};

use anyhow::{anyhow, bail, Context, Result};
use ccsds::spacepacket::TimecodeDecoder;
use ccsds::{framing::SCID, spacepacket::Apid};
use clap::{Parser, Subcommand};
use hifitime::Epoch;
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
        #[arg(short = 'O', long, value_delimiter = ',', value_name = "csv")]
        apid_order: Option<Vec<Apid>>,

        /// A named APID ordering that will override any order provided by --apid-order. The only
        /// value currently supported is jpss-viirs.
        #[arg(short = 'A', long)]
        apid_order_name: Option<String>,

        /// Drop any packets with a time before this time (RFC3339).
        #[arg(short, long, value_parser = parse_timestamp, value_name = "timestamp")]
        from: Option<Epoch>,

        /// Drop any packets with a time after this time (RFC3339).
        #[arg(short, long, value_parser = parse_timestamp, value_name = "timestamp")]
        to: Option<Epoch>,

        /// Drop any packet that has an APID not in this list
        #[arg(short, long, value_delimiter = ',', value_name = "csv")]
        apids: Vec<Apid>,

        /// Delete output file if it already exists
        #[arg(long, action)]
        clobber: bool,

        /// Output file path.
        #[arg(short, long, default_value = "merged.dat", value_name = "path")]
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
    /// Apply various filters to spacepacket files.
    Filter {
        /// Include these apids or apid ranges.
        ///
        /// This accepts a CSV of APIDs as well as ranges of the format <start>-<end>
        /// where start and end are inclusive. For example, you can specify
        /// --include 0,1,2,3,4,5,10,20,30 or --include 0-5,10,20,30
        ///
        /// If used with --exclude, values are first included, then excluded.
        #[arg(short, long, value_name = "csv", value_delimiter = ',')]
        include: Vec<String>,

        /// Exclude these apids or apid ranges.
        ///
        /// This accepts a CSV of APIDs as well as ranges of the format <start>-<end>
        /// where start is inclusive and end is exclusive.
        ///
        /// If used with --include, values are first included, then excluded.
        #[arg(short, long, value_name = "csv", value_delimiter = ',')]
        exclude: Vec<String>,

        /// Only include packets before this time (RFC3339).
        ///
        /// This requires input data to utilize standard CDS times in the secondary
        /// header.
        #[arg(short, long, value_parser = parse_timestamp, value_name = "timestamp")]
        before: Option<Epoch>,

        /// Only include packets after this time (RFC3339).
        ///
        /// This requires input data to utilize standard CDS times in the secondary
        /// header.
        #[arg(short, long, value_parser = parse_timestamp, value_name = "timestamp")]
        after: Option<Epoch>,

        /// Delete output file if it already exists
        #[arg(long, action)]
        clobber: bool,

        /// Output file path.
        #[arg(short, long, default_value = "filtered.dat", value_name = "path")]
        output: PathBuf,

        /// Input spacepacket file.
        input: PathBuf,
    },
    /// View spacecraft information.
    ///
    /// This requires a spacecraft database be available a ./spacecraftdb.json or
    /// ~/.spacecraftdb.json.
    ///
    /// See: https://github.com/bmflynn/spacecraftsdb/releases
    Spacecraft {
        /// Spacecraft identifier
        #[arg(short, long)]
        scid: Option<SCID>,

        /// Path to spacecraft database to merge with built-in spacecrafts.
        #[arg(short, long)]
        db: Option<PathBuf>,
    },
}

fn parse_number_ranges(list: Vec<String>) -> Result<Vec<u32>> {
    let rx = regex::Regex::new(r"^(?:(\d+)|(\d+)-(\d+))$").expect("regex to compile");
    let mut values = Vec::default();
    for (i, s) in list.into_iter().enumerate() {
        let Some(cap) = rx.captures(&s) else {
            bail!("invalid range");
        };
        if cap.len() != 4 {
            bail!("invalid number or range at {i}");
        }

        if cap.get(1).is_some() {
            let x = &cap[1]
                .parse::<u32>()
                .map_err(|_| anyhow!("invalid number value"))?;
            values.push(*x);
        } else {
            let start = &cap[2]
                .parse::<u32>()
                .map_err(|_| anyhow!("invalid range value"))?;
            let end = &cap[3]
                .parse::<u32>()
                .map_err(|_| anyhow!("invalid range value"))?;
            if start >= end {
                bail!("invalid range")
            }
            values.extend(*start..=*end);
        }
    }

    Ok(values)
}

fn parse_timestamp(s: &str) -> Result<Epoch, String> {
    let zult = Epoch::from_str(s);
    if zult.is_err() {
        return Err("Could not parse into an RFC3339 timestamp".to_string());
    }
    Ok(zult.unwrap())
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
            clobber,
            apid_order,
            apid_order_name,
            from,
            to,
            apids,
        } => {
            if !clobber && output.exists() {
                bail!("{output:?} exists; use --clobber");
            }
            info!("merging {inputs:?} to {output:?}");
            let apid_order = match apid_order_name {
                Some(name) => match merge::apid_order(name) {
                    Some(order) => Some(order),
                    None => bail!("{name} is not a valid APID order name"),
                },
                None => Some(apid_order.as_deref().unwrap_or(&Vec::default()).to_vec()),
            };
            let dest = File::create(output)
                .with_context(|| format!("failed to create output {output:?}"))?;

            merge::merge(
                inputs,
                TimecodeDecoder::new(Some(ccsds::timecode::Format::Cds {
                    num_day: 2,
                    num_submillis: 2,
                })),
                dest,
                apid_order,
                *from,
                *to,
                Some(apids),
            )
        }
        Commands::Info {
            input,
            format,
            timecode,
        } => info::info(input, format, timecode),
        Commands::Filter {
            include,
            exclude,
            clobber,
            output,
            input,
            before,
            after,
        } => {
            if !clobber && output.exists() {
                bail!("{output:?} exists; use --clobber");
            }
            let src = File::open(input).context("opening input")?;
            let dest = File::create(output)
                .with_context(|| format!("failed to create output {output:?}"))?;

            let include = parse_number_ranges(include.clone())?
                .iter()
                .filter_map(|v| Apid::try_from(*v).ok())
                .collect::<Vec<Apid>>();
            let exclude = parse_number_ranges(exclude.clone())?
                .iter()
                .filter_map(|v| Apid::try_from(*v).ok())
                .collect::<Vec<Apid>>();

            debug!("including apids {:?}", include);
            debug!("excluding apids {:?}", exclude);
            debug!("before: {:?}", before);
            debug!("after: {:?}", after);

            filter::filter(src, dest, &include, &exclude, *before, *after)
        }
        Commands::Spacecraft { scid, db } => {
            spacecraft::spacecraft_info(db.as_ref(), scid.as_ref().copied(), true, true)
        }
    }
}
