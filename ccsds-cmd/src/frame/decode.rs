use std::{
    collections::HashMap,
    fs::{self, File},
    io::{stdout, Write},
    path::Path,
};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;

use ccsds::{
    config::Config,
    framing::{Frame, Integrity, Pipeline, RsOpts, Vcid},
};
use handlebars::handlebars_helper;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::InputReader;

// size of a single block of RS parity for 223/255
const RS_PARITY_LEN: usize = 32;

#[derive(Default, Debug, Clone, Serialize)]
pub struct Info {
    pub vcid: Vcid,
    pub total_frames: usize,
    pub total_bytes: usize,
    pub missing_frames: usize,

    pub corrected: usize,
    pub uncorrectable: usize,
    pub ok: usize,
    pub error: usize,
    pub not_performed: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Summary {
    pub total_frames: usize,
    pub total_bytes: usize,
    pub missing_frames: usize,

    pub corrected: usize,
    pub uncorrectable: usize,
    pub ok: usize,
    pub error: usize,
    pub not_performed: usize,
    pub with_rs: bool,
    pub vcids: HashMap<Vcid, Info>,
}

impl Summary {
    pub fn collect(&mut self, frame: &Frame) {
        self.total_frames += 1;
        self.total_bytes += frame.data.len();

        let channel = self.vcids.entry(frame.header.vcid).or_default();
        channel.vcid = frame.header.vcid;
        channel.total_frames += 1;
        channel.total_bytes += frame.data.len();
        channel.missing_frames += frame.missing as usize;
        self.missing_frames += frame.missing as usize;
        match &frame.integrity {
            Some(integrity) => match integrity {
                Integrity::Ok => {
                    channel.ok += 1;
                    self.ok += 1;
                }
                Integrity::Corrected => {
                    channel.corrected += 1;
                    self.corrected += 1;
                }
                Integrity::Uncorrectable | Integrity::NotCorrected => {
                    channel.uncorrectable += 1;
                    self.uncorrectable += 1;
                }
                Integrity::Failed => {
                    channel.error += 1;
                    self.error += 1;
                }
            },
            None => {
                channel.not_performed += 1;
                self.not_performed += 1;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, Serialize, Deserialize)]
pub enum FrameType {
    AOS,
}

/// General purpose framing that extracts all contained frames to the provided path.
pub fn frame_aos<O: AsRef<Path>>(
    input: InputReader,
    length: usize,
    pn: bool,
    keep_fill: bool,
    reed_solomon: Option<u8>,
    reed_solomon_detect: bool,
    reed_solomon_correct: bool,
    reed_solomon_virtualfill: usize,
    reed_solomon_threads: Option<usize>,
    reed_solomon_buffersize: usize,
    include: Vec<Vcid>,
    exclude: Vec<Vcid>,
    output: Option<O>,
) -> Result<Summary> {
    let interleave = reed_solomon.unwrap_or_default();
    let sync_block_len = length + RS_PARITY_LEN * interleave as usize;
    info!("using frame/cadu length: {}/{}", length, sync_block_len);
    let mut pipeline = Pipeline::new(sync_block_len);
    if !pn {
        pipeline = pipeline.without_derandomization();
    }
    if let Some(interleave) = reed_solomon {
        let mut opts = RsOpts::new(interleave)
            .with_buffer_size(reed_solomon_buffersize)
            .with_correction(reed_solomon_correct)
            .with_detection(reed_solomon_detect || reed_solomon_correct);
        if reed_solomon_virtualfill != 0 {
            opts = opts.with_virtual_fill(reed_solomon_virtualfill);
        }
        if let Some(num) = reed_solomon_threads {
            opts = opts.with_num_threads(num);
        }
        debug!("rs opts: {:?}", opts);
        pipeline = pipeline.with_rs(opts);
    }

    let mut summary = Summary::default();

    let frames = pipeline.start(input);
    let dst = match output {
        Some(path) => Some(File::create(path).context("creating output")?),
        None => None,
    };

    for frame in frames {
        if frame.is_fill() && !keep_fill {
            continue;
        }
        if !include.is_empty() && !include.contains(&frame.header.vcid) {
            continue;
        }
        if !exclude.is_empty() && exclude.contains(&frame.header.vcid) {
            continue;
        }
        summary.collect(&frame);
        match &frame.integrity {
            Some(Integrity::Uncorrectable | Integrity::NotCorrected | Integrity::Failed) => {
                continue;
            }
            _ => {}
        }

        if let Some(mut fp) = dst.as_ref() {
            fp.write_all(&frame.data[..length])?;
        }
    }

    Ok(summary)
}

pub fn decode<O: AsRef<Path>, S: AsRef<Path>>(
    cfg: &Config,
    input: &str,
    include: Vec<Vcid>,
    exclude: Vec<Vcid>,
    rs_detect: bool,
    rs_correct: bool,
    output: Option<O>,
    summary_path: Option<S>,
) -> Result<()> {
    let input = InputReader::from_str(&input)?;

    let (interleave, virtual_fill) = if let Some(ref rs) = cfg.framing.reed_solomon {
        (
            u8::try_from(rs.interleave)
                .map_err(|_| anyhow!("invalid rs interleave; must be 0 ... 255"))?,
            rs.virtual_fill_length.unwrap_or_default(),
        )
    } else {
        (0, 0)
    };
    let summary = frame_aos(
        input,
        cfg.framing.length,
        cfg.framing.pseudo_noise.is_some(),
        true,
        if interleave == 0 {
            None
        } else {
            Some(interleave)
        }, // only do rs if interleave set
        rs_detect,
        rs_correct,
        virtual_fill,
        None,
        100,
        include,
        exclude,
        output.as_ref(),
    )?;

    if let Some(path) = summary_path {
        let content = render_json_summary(&summary).context("rendering summary")?;
        fs::write(path, content).context("writing JSON summary")?;
    }
    write_text_summary(stdout(), &summary)
}

pub fn render_json_summary(summary: &Summary) -> Result<String> {
    serde_json::to_string_pretty(&summary).context("serde")
}

const TEXT_TEMPLATE: &str = r#"======================================================================================================
Frames:        {{ summary.total_frames }}
Bytes:         {{ summary.total_bytes }} 
Missing:       {{ summary.missing_frames}}
Corrected:     {{ summary.corrected }}
Uncorrectable: {{ summary.uncorrectable }}
Ok:            {{ summary.ok }}
Error:         {{ summary.error }}
NotPerformed:  {{ summary.not_performed }}
------------------------------------------------------------------------------------------------------
VCID  Frames      Bytes       Missing     Corrected   Uncorr.     Ok          Error       NotPerf.
------------------------------------------------------------------------------------------------------
{{ #each vcids }}
{{ lpad 4 this.vcid }}
{{~ lpad 12 this.total_frames }}
{{~ lpad 12 this.total_bytes }}
{{~ lpad 12 this.missing_frames }}
{{~ lpad 12 this.corrected }}
{{~ lpad 12 this.uncorrectable }}
{{~ lpad 12 this.ok }}
{{~ lpad 12 this.error }}
{{~ lpad 12 this.not_performed }}
{{/each}}
"#;

pub fn write_text_summary<W: Write>(mut w: W, summary: &Summary) -> Result<()> {
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
    assert!(hb.register_template_string("main", TEXT_TEMPLATE).is_ok());

    let mut ordered_vcids: Vec<Info> = summary.vcids.values().cloned().collect();
    ordered_vcids.sort_unstable_by(|a, b| a.vcid.cmp(&b.vcid));

    #[derive(Debug, Serialize)]
    struct Data {
        vcids: Vec<Info>,
        summary: Summary,
    }

    let content = hb
        .render(
            "main",
            &Data {
                vcids: ordered_vcids,
                summary: summary.clone(),
            },
        )
        .context("rendering text")?;
    w.write_all(content.as_bytes()).context("writing tempalte")
}
