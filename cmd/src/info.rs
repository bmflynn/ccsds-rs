use anyhow::{Context, Result};
use ccsds::Apid;
use chrono::{DateTime, TimeDelta, Utc};
use handlebars::handlebars_helper;
use serde::{Serialize, Serializer};
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
    #[serde(serialize_with="serialize_t")]
    first_packet_time: Option<u64>,
    #[serde(serialize_with="serialize_t")]
    last_packet_time: Option<u64>,
    #[serde(serialize_with="serialize_dur")]
    duration: u64,
}

#[derive(Debug, Clone, Serialize)]
struct Info {
    filename: String,
    summary: Summary,
    apids: HashMap<Apid, Summary>,
}

fn summarize(fpath: &Path, tc_format: &TCFormat) -> Result<Info> {
    let reader = std::fs::File::open(fpath).context("opening input")?;
    let packets = ccsds::read_packets(reader).filter_map(Result::ok);
    let time_decoder: Option<&dyn ccsds::TimeDecoder> = match tc_format {
        TCFormat::Cds => Some(&ccsds::CDSTimeDecoder),
        // TCFormat::EosCuc => unimplemented!(),
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
            ccsds::missing_packets(cur, *last)
        };
        last_seqid.insert(packet.header.apid, packet.header.sequence_id);
        summary.missing_packets += missing as usize;

        let apid = apids.entry(packet.header.apid).or_default();
        apid.total_packets += 1;
        apid.missing_packets += missing as usize;

        if !packet.header.has_secondary_header {
            continue;
        }

        if let Some(tc) = time_decoder {
            if let Some(t) = tc.decode_time(&packet) {
                summary.first_packet_time = summary
                    .first_packet_time
                    .map_or(Some(t), |cur| Some(cmp::min(t, cur)));
                summary.last_packet_time = summary
                    .last_packet_time
                    .map_or(Some(t), |cur| Some(cmp::max(t, cur)));
                if summary.first_packet_time.is_some() && summary.last_packet_time.is_some() {
                    summary.duration =
                        summary.last_packet_time.unwrap() - summary.first_packet_time.unwrap();
                }

                apid.first_packet_time = apid
                    .first_packet_time
                    .map_or(Some(t), |cur| Some(cmp::min(t, cur)));
                apid.last_packet_time = apid
                    .last_packet_time
                    .map_or(Some(t), |cur| Some(cmp::max(t, cur)));
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

fn format_t<T>(t: T) -> String
where
    T: std::convert::Into<i64>,
{
    DateTime::<Utc>::from_timestamp_micros(i64::try_from(t).unwrap()).map_or(String::new(), |dt| {
        dt.format("%Y-%m-%dT%H:%M:%S.%6fZ").to_string()
    })
}

fn serialize_t<S>(t: &Option<u64>, s: S) -> std::result::Result<S::Ok, S::Error> 
where
    S: Serializer,
{
    if t.is_none() {
        s.serialize_str("")
    } else {
        let t = i64::try_from(t.unwrap()).unwrap();
        s.serialize_str(&format_t(t))
    }
}

fn format_dur<T>(t: T) -> String
where
    T: std::convert::Into<u64>,
{
    let t = u64::try_from(t).unwrap();
    let secs = t / 1_000_000;
    let nanos = u32::try_from(t - secs * 1_000_000).unwrap() * 1_000;
    let d = TimeDelta::new(i64::try_from(secs).unwrap(), nanos).unwrap();
    d.to_string()
}


fn serialize_dur<S>(t: &u64, s: S) -> std::result::Result<S::Ok, S::Error> 
where
    S: Serializer,
{
    s.serialize_str(&format_dur(*t))
}

fn render_text(info: &Info) -> Result<String> {
    handlebars_helper!(left_pad: |num: u64, v: Json| {
        let v = if let serde_json::Value::String(s) = v {
            s.to_owned()
        } else {
            v.to_string()
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
===========================================================================================
First:    {{ summary.first_packet_time }}
Last:     {{ summary.last_packet_time }} 
Duration: {{ summary.duration }}
APIDS:    {{ #each apids }}{{ @key }}{{ #if @last }}{{ else }}, {{ /if }}{{ /each }}
Count:    {{ summary.total_packets }}
Missing:  {{ summary.missing_packets }}
-------------------------------------------------------------------------------------------
APID    First                        Last                           Count   Missing
-------------------------------------------------------------------------------------------
{{ #each apids }}{{ lpad 6 @key }}  {{ first_packet_time }}  {{ last_packet_time }}   {{ lpad 6 total_packets }}   {{ lpad 7 missing_packets }}
{{/each }}
";
