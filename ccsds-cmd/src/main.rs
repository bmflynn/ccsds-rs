mod frame;
mod packet;

use std::fs;
use std::io::{stdout, BufReader, Read};
use std::net::TcpStream;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs::File, io::stderr};

use ccsds::config::{self, Config};
use ccsds::framing::Vcid;
use ccsds::spacepacket::Apid;

use anyhow::{anyhow, bail, Context, Result};
use ccsds::spacepacket::timecode::PacketApidTimeDecoder;
use clap::{Parser, Subcommand, ValueEnum};
use hifitime::Epoch;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Spacepacket commands
    Packets(PacketArgs),
    /// Frame commands
    Frames(FrameArgs),
}

#[derive(Debug, clap::Args)]
struct PacketArgs {
    #[command(subcommand)]
    command: PacketCommands,
}

#[derive(Debug, clap::Args)]
struct FrameArgs {
    #[command(subcommand)]
    command: FrameCommands,
}

#[derive(Debug, Subcommand)]
enum PacketCommands {
    /// Merge multiple spacepacket files.
    ///
    /// Contained packets must have an 8 byte CDS timecode at the start of the packet
    /// secondary header.
    ///
    /// The merge process will reorder packets by time and APID. To write the merged
    /// packets in a specific order see --apid-order.
    Merge {
        /// Path to spacecraft config file.
        #[arg(short = 'c', long = "config")]
        config: PathBuf,

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
        format: crate::packet::Format,

        /// Path to spacecraft config file.
        #[arg(short = 'c', long = "config")]
        config: PathBuf,
    },
    /// Apply various filters to spacepacket files.
    Filter {
        /// Path to spacecraft config file.
        #[arg(short = 'c', long = "config")]
        config: Option<PathBuf>,

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

    /// Extract packets from frame stream
    Demux {
        /// Path to spacecraft config file.
        #[arg(short = 'c', long = "config")]
        config: PathBuf,

        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Input file containing decoded frames. The input must not include any
        /// ASM or RS parity bytes.
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
#[command()]
enum FrameCommands {
    /// Decode frames from an input stream of CADUs.
    Decode {
        /// Path to spacecraft config file.
        #[arg(short = 'c', long = "config")]
        config: PathBuf,

        /// Enable RS correction (implies rs-detect).
        #[arg(long, default_value_t = false)]
        rs_correct: bool,
        /// Enable RS detection only, no correction.
        #[arg(long, default_value_t = false)]
        rs_detect: bool,

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
    /// Synchronize a bit stream.
    Sync {
        /// Do not include ASM in output
        #[arg(short, long, default_value_t = false)]
        no_asm: bool,

        /// Applly PN algorithm to all non-ASM bytes before writing.
        ///
        /// This effectively toggles the pseudo-random noise in the output.
        #[arg(short, long, default_value_t = false)]
        pn: bool,

        /// Output file path for syncronized data. If not specified only print the position of
        /// located ASMs.
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Print data block start byte and bit offset
        #[arg(short, long, default_value_t = false)]
        verbose: bool,

        /// Length of a CADU, not including the sync marker.
        block_len: usize,

        /// Input file path
        input: PathBuf,
    },
    Info {
        /// Path to spacecraft config file.
        #[arg(short = 'c', long = "config")]
        config: PathBuf,

        /// Input file path
        input: PathBuf,
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
        Commands::Frames(args) => match &args.command {
            FrameCommands::Decode {
                config,
                rs_detect,
                rs_correct,
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

                let input = InputReader::from_str(&input)?;

                let cfg = Config::read(config)?;

                let (interleave, virtual_fill) = if let Some(rs) = cfg.framing.reed_solomon {
                    (
                        u8::try_from(rs.interleave)
                            .map_err(|_| anyhow!("invalid rs interleave; must be 0 ... 255"))?,
                        rs.virtual_fill_length.unwrap_or_default(),
                    )
                } else {
                    (0, 0)
                };
                let summary = frame::frame_aos(
                    input,
                    cfg.framing.length,
                    cfg.framing.pseudo_noise.is_some(),
                    true,
                    if interleave == 0 {
                        None
                    } else {
                        Some(interleave)
                    }, // only do rs if interleave set
                    *rs_detect,
                    *rs_correct,
                    virtual_fill,
                    None,
                    100,
                    include,
                    exclude,
                    output.as_ref(),
                )?;

                if let Some(path) = summary_path {
                    let content =
                        frame::render_json_summary(&summary).context("rendering summary")?;
                    fs::write(path, content).context("writing JSON summary")?;
                }
                frame::write_text_summary(stdout(), &summary)
            }
            FrameCommands::Sync {
                input,
                block_len,
                output,
                pn,
                no_asm,
                verbose,
            } => frame::synchronize(
                input.clone(),
                *block_len,
                *pn,
                *no_asm,
                output.clone(),
                *verbose,
            ),
            FrameCommands::Info { config, input } => {
                let cfg = config::Config::read(config)?;
                crate::frame::info(input, &cfg)
            }
        },
        Commands::Packets(args) => match &args.command {
            PacketCommands::Merge {
                config,
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
                    Some(name) => match crate::packet::apid_order(&name) {
                        Some(order) => Some(order),
                        None => bail!("{name} is not a valid APID order name"),
                    },
                    None => Some(apid_order.as_deref().unwrap_or(&Vec::default()).to_vec()),
                };
                let dest = File::create(output)
                    .with_context(|| format!("failed to create output {output:?}"))?;

                let cfg = config::Config::read(config)?;

                let time_decoder = PacketApidTimeDecoder::default().with_config(&cfg)?;

                crate::packet::merge(
                    &inputs,
                    time_decoder,
                    dest,
                    apid_order,
                    *from,
                    *to,
                    Some(&apids),
                )
            }
            PacketCommands::Info {
                input,
                format,
                config,
            } => {
                let cfg = Config::read(config)?;
                let timecodes = match PacketApidTimeDecoder::default().with_config(&cfg) {
                    Ok(tc) => Some(tc),
                    Err(err) => {
                        warn!(%err, "failed to configured timecodes");
                        None
                    }
                };
                crate::packet::info(&input, &format, timecodes)
            }
            PacketCommands::Filter {
                include,
                exclude,
                clobber,
                output,
                input,
                before,
                after,
                config,
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

                let time_decoder = match config {
                    Some(path) => {
                        let cfg = Config::read(path)?;
                        let decoder = PacketApidTimeDecoder::default().with_config(&cfg)?;
                        Some(decoder)
                    }
                    None => None,
                };

                crate::packet::filter(src, dest, &include, &exclude, *before, *after, time_decoder)
            }
            PacketCommands::Diff {
                left,
                right,
                verbose,
            } => crate::packet::diff(left, right, *verbose),
            PacketCommands::Demux {
                input,
                output,
                config,
            } => {
                let cfg = Config::read(config)?;

                crate::packet::demux_packets(input, cfg, output.as_ref())
            }
        },
    }
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
