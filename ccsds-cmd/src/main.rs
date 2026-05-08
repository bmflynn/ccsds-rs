mod config;
mod diff;
mod filter;
mod frame;
mod info;
mod merge;

use std::fs;
use std::io::{stdout, BufReader, Read};
use std::net::TcpStream;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs::File, io::stderr};

use anyhow::{anyhow, bail, Context, Result};
use ccsds::framing::Vcid;
use ccsds::spacepacket::Apid;
use ccsds::spacepacket::TimecodeDecoder;
use clap::{Parser, Subcommand, ValueEnum};
use hifitime::Epoch;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SummaryFormat {
    JSON,
    TXT,
}

#[derive(Debug)]
pub enum InputReader {
    Stdin(BufReader<std::io::Stdin>),
    File(BufReader<std::fs::File>),
    TCP(BufReader<std::net::TcpStream>),
}

impl InputReader {
    fn from_str(s: &str) -> Result<InputReader> {
        if s == "-" {
            return Ok(InputReader::Stdin(BufReader::new(std::io::stdin())));
        }
        if std::fs::exists(s).unwrap_or_default() {
            return Ok(InputReader::File(BufReader::new(File::open(s)?)));
        }
        let conn = TcpStream::connect(s).context("failed to connect")?;
        return Ok(InputReader::TCP(BufReader::new(conn)));
    }
}

impl Read for InputReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            InputReader::Stdin(r) => r.read(buf),
            InputReader::File(r) => r.read(buf),
            InputReader::TCP(r) => r.read(buf),
        }
    }
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
        /// This accepts a CSV of APIDs as well as ranges of the format `<start>-<end>`
        /// where start and end are inclusive. For example, you can specify
        /// --include 0,1,2,3,4,5,10,20,30 or --include 0-5,10,20,30
        ///
        /// If used with --exclude, values are first included, then excluded.
        #[arg(short, long, value_name = "csv", value_delimiter = ',')]
        include: Vec<String>,

        /// Exclude these apids or apid ranges.
        ///
        /// This accepts a CSV of APIDs as well as ranges of the format `<start>-<end>`
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

    /// Decode frames from an input stream of CADUs.
    Framing {
        /// Spacecraft framing JSON config file. If provided assiciated flags are ignored.
        ///
        /// JSON config format:
        /// {"asm": [<int>,...],
        ///  "scid": <u16>,
        ///  "type": "aos",
        ///  "length": <int>,
        ///  "pn": bool,
        ///  "rs": {"interleave": <int>, "virtualfill": <int>}
        /// }
        #[arg(short = 'c', long = "config")]
        config: Option<PathBuf>,
        /// Type of the contained frames
        #[arg(short = 't', long = "type", default_value = "aos")]
        frame_type: frame::FrameType,
        /// Frame length not including any reed-solomon parity or cadu attached sync marker
        /// bytes.
        #[arg(short, long, value_name = "NUM", default_value_t = 0)]
        length: usize,
        /// Remove pseudo-noise
        #[arg(short='N', long, action=clap::ArgAction::SetTrue)]
        pn: bool,
        /// Don't drop fill frames
        #[arg(long, action=clap::ArgAction::SetTrue)]
        keep_fill: bool,

        /// Enables reed-solomon handling with this interleave.
        #[arg(short, long, value_name = "INTERLEAVE")]
        rs: Option<u8>,
        /// If reed-solomon is enabled, perform error detection. Ignored unless --rs.
        #[arg(long, action=clap::ArgAction::SetTrue)]
        rs_detect: bool,
        /// If reed-solomon is enabled, perform error correction. Ignored unless --rs, implies
        /// --rs-detect
        #[arg(short='C', long, action=clap::ArgAction::SetTrue)]
        rs_correct: bool,
        /// Number of reed-solomon virtual-fill bytes. Ignored unless --rs.
        #[arg(short = 'V', long, value_name = "NUM", default_value = "0")]
        rs_virtualfill: usize,
        /// Number of threads to use for reed-solomon. Defaults to all available.
        #[arg(long, value_name = "NUM")]
        rs_threads: Option<usize>,
        /// Number of frames to keep waiting in memory.
        #[arg(long, value_name = "NUM", default_value = "50")]
        rs_buffersize: usize,

        /// Include these vcids or vcid ranges. If not specified, include all.
        ///
        /// This accepts a CSV of VCIDs as well as ranges of the format `<start>-<end>`
        /// where start and end are inclusive. For example, you can specify
        /// --include 0,1,2,3,4,5,10,20,30 or --include 0-5,10,20,30
        ///
        /// If used with --exclude, values are first included, then excluded.
        #[arg(long, value_name = "csv", value_delimiter = ',')]
        include: Vec<String>,

        /// Exclude these vcids or vcid ranges.
        ///
        /// This accepts a CSV of vcids as well as ranges of the format `<start>-<end>`
        /// where start is inclusive and end is exclusive.
        ///
        /// If used with --include, values are first included, then excluded.
        #[arg(short, long, value_name = "csv", value_delimiter = ',')]
        exclude: Vec<String>,

        /// Output file path, or '-' for stdout. If not specified only print the summary.
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Write a JSON summary of the decode.
        #[arg(short, long)]
        summary: Option<PathBuf>,

        /// Input file path
        input: String,
    },

    /// Difference 2 packet files.
    ///
    /// Packet differences are based on APID, sequence number, and CRC (not including the packet
    /// header).
    Diff {
        left: PathBuf,
        right: PathBuf,
        /// Show details on specific missing packets
        #[arg(short, long)]
        verbose: bool,
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
                TimecodeDecoder::new(ccsds::timecode::Format::Cds {
                    num_day: 2,
                    num_submillis: 2,
                }),
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
        Commands::Diff {
            left,
            right,
            verbose,
        } => crate::diff::diff(left, right, *verbose),
        Commands::Framing {
            config,
            frame_type: _,
            mut length,
            mut pn,
            keep_fill,
            mut rs,
            rs_detect,
            rs_correct,
            mut rs_virtualfill,
            rs_threads,
            rs_buffersize,
            include,
            exclude,
            input,
            output,
            summary: summary_path,
        } => {
            let include = parse_number_ranges(include.clone())?
                .iter()
                .filter_map(|v| Vcid::try_from(*v).ok())
                .collect::<Vec<Vcid>>();
            let exclude = parse_number_ranges(exclude.clone())?
                .iter()
                .filter_map(|v| Vcid::try_from(*v).ok())
                .collect::<Vec<Vcid>>();

            let input = InputReader::from_str(input)?;

            if let Some(path) = config {
                let config = Config::read(path)?;
                length = config.length;
                // frame_type = config.frame_type;
                pn = config.pn;
                if let Some(cfg) = config.rs {
                    rs = Some(cfg.interleave as u8);
                    rs_virtualfill = cfg.virtualfill;
                }
            }

            if length == 0 {
                bail!("length cannot be 0")
            }

            let summary = frame::frame_aos(
                input,
                length,
                pn,
                *keep_fill,
                rs,
                *rs_detect,
                *rs_correct,
                rs_virtualfill,
                *rs_threads,
                *rs_buffersize,
                include,
                exclude,
                output.as_ref(),
            )?;

            if let Some(path) = summary_path {
                let content = frame::render_json_summary(&summary).context("rendering summary")?;
                fs::write(path, content).context("writing JSON summary")?;
            }
            frame::write_text_summary(stdout(), &summary)
        }
    }
}
