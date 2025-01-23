use anyhow::{Context, Result};
use ccsds::spacepacket::{decode_packets, missing_packets, Apid, TimecodeDecoder};
use handlebars::handlebars_helper;
use hifitime::{Duration, Epoch};
use serde::Serialize;
use std::{
    cmp,
    collections::{hash_map::Entry, HashMap},
    io::{stdout, Write},
    path::Path,
};
use tracing::debug;

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
    // EosCuc,
    None,
}

impl clap::ValueEnum for TCFormat {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::Cds,
            // Self::EosCuc,
            Self::None,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Cds => Some(clap::builder::PossibleValue::new("cds")),
            // Self::EosCuc => Some(clap::builder::PossibleValue::new("eoscuc")),
            Self::None => Some(clap::builder::PossibleValue::new("none")),
        }
    }
}

#[derive(Default, Debug, Clone, Serialize)]
struct Summary {
    total_packets: usize,
    missing_packets: usize,
    first_packet_time: Option<Epoch>,
    last_packet_time: Option<Epoch>,
    duration: Duration,
}

#[derive(Debug, Clone, Serialize)]
struct Info {
    filename: String,
    summary: Summary,
    apids: HashMap<Apid, Summary>,
}

fn new_cds_decoder() -> TimecodeDecoder {
    TimecodeDecoder::new(ccsds::timecode::Format::Cds {
        num_day: 2,
        num_submillis: 2,
    })
}

fn summarize(fpath: &Path, tc_format: &TCFormat) -> Result<Info> {
    let reader = std::fs::File::open(fpath).context("opening input")?;
    let packets = decode_packets(reader).filter_map(Result::ok);
    let time_decoder: Option<TimecodeDecoder> = match tc_format {
        TCFormat::Cds => Some(new_cds_decoder()),
        TCFormat::None => None,
    };

    let mut last_seqid: HashMap<Apid, u16> = HashMap::default();
    let mut apids: HashMap<Apid, Summary> = HashMap::default();
    let mut summary = Summary::default();

    for packet in packets {
        summary.total_packets += 1;

        let missing = if let Entry::Vacant(e) = last_seqid.entry(packet.header.apid) {
            e.insert(packet.header.sequence_id);
            0
        } else {
            let cur = packet.header.sequence_id;
            let last = last_seqid.get(&packet.header.apid).unwrap(); // we know it exists
            missing_packets(cur, *last)
        };
        last_seqid.insert(packet.header.apid, packet.header.sequence_id);
        summary.missing_packets += missing as usize;

        let apid = apids.entry(packet.header.apid).or_default();
        apid.total_packets += 1;
        apid.missing_packets += missing as usize;

        if !packet.header.has_secondary_header {
            continue;
        }

        if let Some(ref time_decoder) = time_decoder {
            if let Ok(epoch) = time_decoder.decode(&packet) {
                summary.first_packet_time = summary
                    .first_packet_time
                    .map_or(Some(epoch), |cur| Some(cmp::min(epoch, cur)));
                summary.last_packet_time = summary
                    .last_packet_time
                    .map_or(Some(epoch), |cur| Some(cmp::max(epoch, cur)));
                if summary.first_packet_time.is_some() && summary.last_packet_time.is_some() {
                    summary.duration =
                        summary.last_packet_time.unwrap() - summary.first_packet_time.unwrap();
                }

                apid.first_packet_time = apid
                    .first_packet_time
                    .map_or(Some(epoch), |cur| Some(cmp::min(epoch, cur)));
                apid.last_packet_time = apid
                    .last_packet_time
                    .map_or(Some(epoch), |cur| Some(cmp::max(epoch, cur)));
                if apid.first_packet_time.is_some() && apid.last_packet_time.is_some() {
                    apid.duration =
                        apid.last_packet_time.unwrap() - apid.first_packet_time.unwrap();
                }
            } else {
                debug!("failed to decode time from {:?}", packet.header);
            }
        }
    }

    Ok(Info {
        filename: fpath.to_string_lossy().to_string(),
        summary,
        apids,
    })
}

pub fn info(fpath: &Path, format: &Format, tc_format: &TCFormat) -> Result<()> {
    let info = summarize(fpath, tc_format)?;

    match format {
        Format::Json => {
            serde_json::to_writer_pretty(stdout(), &info).context("serializing to json")
        }
        Format::Text => {
            let data = render_text(&info).context("serializing info")?;
            stdout()
                .write_all(str::as_bytes(&data))
                .context("writing to stdout")
        }
    }
}

fn render_text(info: &Info) -> Result<String> {
    handlebars_helper!(left_pad: |num: u64, v: Json| {
        let v = match v {
            serde_json::Value::String(s) => s.to_owned(),
            serde_json::Value::Null => String::new(),
            _ => v.to_string()
        };
        let mut num: usize = usize::try_from(num).unwrap();
        if num < v.len() {
            num = v.len();
        }
        let mut s = String::new();
        let padding = num - v.len();
        for _ in 0..padding {
            s.push(' ');
        }
        s.push_str(&v);
        s
    });
    let mut hb = handlebars::Handlebars::new();
    hb.register_helper("lpad", Box::new(left_pad));
    assert!(hb.register_template_string("info", TEXT_TEMPLATE).is_ok());

    hb.render("info", &info).context("rendering text")
}

const TEXT_TEMPLATE: &str = r"{{ filename }}
===============================================================================================
First:    {{ summary.first_packet_time }}
Last:     {{ summary.last_packet_time }} 
Duration: {{ summary.duration }}
APIDS:    {{ #each apids }}{{ @key }}{{ #if @last }}{{ else }}, {{ /if }}{{ /each }}
Count:    {{ summary.total_packets }}
Missing:  {{ summary.missing_packets }}
-----------------------------------------------------------------------------------------------
APID    First                              Last                                 Count   Missing
-----------------------------------------------------------------------------------------------
{{ #each apids }}{{ lpad 6 @key }}  {{ lpad 33 first_packet_time }}  {{ lpad 33 last_packet_time }}   {{ lpad 6 total_packets }}   {{ lpad 7 missing_packets }}
{{/each }}
";
