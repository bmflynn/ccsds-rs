use anyhow::{Context, Result};
use ccsds::framing::{synchronize, Integrity, Pipeline, Vcid, ASM};
use handlebars::handlebars_helper;
use serde::Serialize;
use spacecrafts::FramingConfig;
use std::{
    collections::HashMap,
    fs::File,
    io::{stdout, BufReader, Write},
    path::Path,
};
use tracing::{debug, warn};

pub fn sync(srcpath: &Path, dstpath: &Path, block_size: usize) -> Result<()> {
    let src = BufReader::new(File::open(srcpath).context("opening source")?);
    let mut dst = File::create(dstpath).context("creating dest")?;

    for cadu in synchronize(src, block_size) {
        dst.write_all(&ASM)?;
        dst.write_all(&cadu.data)?;
    }

    Ok(())
}

pub fn frame(
    srcpath: &Path,
    dstpath: &Path,
    config: FramingConfig,
    include: Vec<Vcid>,
    exclude: Vec<Vcid>,
) -> Result<()> {
    let mut pipeline = Pipeline::new();

    if config.pseudo_noise.is_none() {
        pipeline = pipeline.without_pn();
    }

    if let Some(rs_config) = config.reed_solomon {
        pipeline = pipeline.with_default_rs(rs_config.interleave, rs_config.virtual_fill_length);
    }

    let frame_len = config.length;
    let src = BufReader::new(File::open(srcpath).context("opening source")?);
    let frames = pipeline.start(src, frame_len);

    let mut dst = File::create(dstpath).context("creating dest")?;

    for (idx, frame) in frames.enumerate() {
        if !include.is_empty() && !include.contains(&frame.header.vcid) {
            continue;
        }
        if !exclude.is_empty() && exclude.contains(&frame.header.vcid) {
            continue;
        }

        let mpdu = frame
            .mpdu(config.insert_zone_length, config.trailer_length)
            .unwrap();

        debug!(
            "{:?} {:?} missing={} integrity={}",
            frame.header,
            mpdu,
            frame.missing,
            frame
                .integrity
                .map_or_else(|| "None".to_string(), |i| format!("{i:?}"))
        );

        if frame.data.len() != frame_len {
            warn!(
                "expected frame length={frame_len} at frame idx {idx}, got {}; dropping",
                frame.data.len()
            );
            continue;
        }

        dst.write_all(&frame.data)?;
    }

    Ok(())
}

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

#[derive(Default, Debug, Clone, Serialize)]
struct Summary {
    total_frames: usize,
    total_bytes: usize,
    missing_frames: usize,

    corrected: usize,
    uncorrectable: usize,
    ok: usize,
    error: usize,
    not_performed: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Info {
    filename: String,
    summary: Summary,
    vcids: Vec<(Vcid, Summary)>,
}

pub fn info(config: FramingConfig, fpath: &Path, format: &Format) -> Result<()> {
    let mut pipeline = Pipeline::new();

    if config.pseudo_noise.is_none() {
        pipeline = pipeline.without_pn();
    }

    if let Some(rs_config) = config.reed_solomon {
        pipeline = pipeline.with_default_rs(rs_config.interleave, rs_config.virtual_fill_length);
    }

    let frame_len = config.length;
    let src = BufReader::new(File::open(fpath).context("opening source")?);
    let frames = pipeline.start(src, frame_len);

    let mut info = Info {
        filename: fpath.file_name().unwrap().to_string_lossy().to_string(),
        summary: Summary::default(),
        vcids: Vec::default(),
    };
    let mut vcids: HashMap<Vcid, Summary> = HashMap::default();
    for frame in frames {
        debug!("{:?}", frame.header);
        info.summary.total_frames += 1;
        info.summary.total_bytes += frame.data.len();

        let sum = vcids.entry(frame.header.vcid).or_default();
        sum.total_frames += 1;
        sum.total_bytes += frame.data.len();
        match frame.integrity {
            Some(integrity) => match integrity {
                Integrity::Ok => {
                    sum.ok += 1;
                    info.summary.ok += 1;
                }
                Integrity::Corrected => {
                    sum.corrected += 1;
                    info.summary.corrected += 1;
                }
                Integrity::Uncorrectable => {
                    sum.uncorrectable += 1;
                    info.summary.uncorrectable += 1;
                }
                Integrity::Skipped => {
                    sum.not_performed += 1;
                    info.summary.not_performed += 1;
                }
            },
            None => {
                sum.not_performed += 1;
                info.summary.not_performed += 1;
            }
        }
    }

    info.vcids = vcids.into_iter().collect();
    info.vcids.sort_by_key(|(k, _)| *k);

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
    let mut hb = handlebars::Handlebars::new();

    handlebars_helper!(right_pad: |num: u64, v: Json| {
        let v = match v {
            serde_json::Value::String(s) => s.to_owned(),
            serde_json::Value::Null => String::new(),
            _ => v.to_string()
        };
        let mut num: usize = usize::try_from(num).unwrap();
        if num < v.len() {
            num = v.len();
        }
        let mut s = v.to_string();
        let padding = num - v.len();
        for _ in 0..padding {
            s.push(' ');
        }
        s
    });
    hb.register_helper("rpad", Box::new(right_pad));
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
    hb.register_helper("lpad", Box::new(left_pad));
    assert!(hb.register_template_string("info", TEXT_TEMPLATE).is_ok());

    hb.render("info", &info).context("rendering text")
}

const TEXT_TEMPLATE: &str = r#"{{ filename }}
===============================================================================================
VCIDs:    {{ #each vcids }}{{ this.[0] }}{{ #if @last }}{{ else }}, {{ /if }}{{ /each }}
Count:    {{ summary.total_frames }}
Missing:  {{ summary.missing_frames }}
Integrity: 
    Ok:         {{ summary.ok }}
    Corrected:  {{ summary.corrected }}
    Failed:     {{ summary.uncorrectable }}
    Error:      {{ summary.error }}
    NotChecked: {{ summary.not_performed }}
-----------------------------------------------------------------------------------------------
VCID     Count   Missing            Bytes        Ok Corrected    Failed     Error  NotChecked
-----------------------------------------------------------------------------------------------
{{ #each vcids }}{{ lpad 4 this.[0] }}  {{ #with this.[1] }}{{ lpad 8 total_frames }}  {{ lpad 8 missing_frames }}  {{ lpad 15 total_bytes }}  {{ lpad 8 ok }}  {{ lpad 8 corrected }}  {{ lpad 8 uncorrectable }}  {{ lpad 8 error }}  {{ lpad 8 not_performed }}{{ /with }}
{{/each }}
"#;
