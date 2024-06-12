use anyhow::{Context, Result};
use ccsds::{Apid, TimeDecoder};
use serde::Serialize;
use std::{collections::HashMap, io::stdout, path::Path};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub enum Format {
    Json,
    Text,
}

impl clap::ValueEnum for Format {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Json, Self::Text]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Json => Some(clap::builder::PossibleValue::new("json")),
            Self::Text => Some(clap::builder::PossibleValue::new("text")),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TCFormat {
    Cds,
    EosCuc,
    None,
}

impl clap::ValueEnum for TCFormat {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Cds, Self::EosCuc, Self::None]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Cds => Some(clap::builder::PossibleValue::new("cds")),
            Self::EosCuc => Some(clap::builder::PossibleValue::new("eoscuc")),
            Self::None => Some(clap::builder::PossibleValue::new("none")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Summary {
    total_packets: usize,
    missing_packets: usize,
    first_packet_time: Option<u64>,
    last_packet_time: Option<u64>,
    duration: u64,
}

impl Default for Summary {
    fn default() -> Self {
        Self {
            total_packets: 0,
            missing_packets: 0,
            first_packet_time: None,
            last_packet_time: None,
            duration: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Info {
    summary: Summary,
    apids: HashMap<Apid, Summary>,
}

fn summarize(fpath: &Path, tc_format: &TCFormat) -> Result<Info> {
    let reader = std::fs::File::open(fpath).context("opening input")?;
    let packets = ccsds::read_packets(reader).filter_map(Result::ok);
    let time_decoder: Option<&dyn ccsds::TimeDecoder> = match tc_format {
        TCFormat::Cds => Some(&ccsds::CDSTimeDecoder),
        TCFormat::EosCuc => unimplemented!(),
        TCFormat::None => None,
    };

    let mut last_seqid: Option<u16> = None;
    let mut apids: HashMap<Apid, Summary> = HashMap::default();
    let mut summary = Summary::default();

    for packet in packets {
        summary.total_packets += 1;

        let missing = last_seqid.map_or(0, |last| {
            ccsds::missing_packets(packet.header.sequence_id, last)
        });
        last_seqid = Some(packet.header.sequence_id);

        summary.missing_packets += missing as usize;
        let apid = apids.entry(packet.header.apid).or_default();
        apid.total_packets += 1;

        if let Some(tc) = time_decoder {
            match tc.decode_time(&packet) {
                Some(t) => {
                    summary.first_packet_time =
                        Some(if let Some(last) = summary.first_packet_time {
                            if t < last {
                                t
                            } else {
                                last
                            }
                        } else {
                            t
                        });
                    summary.last_packet_time = Some(if let Some(last) = summary.last_packet_time {
                        if t > last {
                            t
                        } else {
                            last
                        }
                    } else {
                        t
                    });
                    if summary.first_packet_time.is_some() && summary.last_packet_time.is_some() {
                        summary.duration =
                            summary.last_packet_time.unwrap() - summary.first_packet_time.unwrap();
                    }
                    apid.first_packet_time = Some(if let Some(last) = apid.first_packet_time {
                        if t < last {
                            t
                        } else {
                            last
                        }
                    } else {
                        t
                    });
                    apid.last_packet_time = Some(if let Some(last) = apid.last_packet_time {
                        if t > last {
                            t
                        } else {
                            last
                        }
                    } else {
                        t
                    });
                    if apid.first_packet_time.is_some() && apid.last_packet_time.is_some() {
                        apid.duration =
                            apid.last_packet_time.unwrap() - apid.first_packet_time.unwrap();
                    }
                }
                None => debug!("failed to decode time from {:?}", packet.header),
            }
        }
    }

    Ok(Info { summary, apids })
}

pub fn info(fpath: &Path, format: &Format, tc_format: &TCFormat) -> Result<()> {
    let summary = summarize(fpath, tc_format)?;

    match format {
        Format::Json => {
            serde_json::to_writer_pretty(stdout(), &summary).context("serializing to json")
        }
        Format::Text => unimplemented!(),
    }
}
