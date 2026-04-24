use std::{
    collections::HashMap,
    fs::File,
    io::{stdout, Write},
    path::Path,
};

use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;

use ccsds::framing::{Integrity, Pipeline, RsOpts, Vcid};
use serde::Serialize;
use tracing::warn;

use crate::{InputReader, SummaryFormat};

const FEC_LEN: usize = 2;
const OCF_LEN: usize = 4;
const RS_PARITY_LEN: usize = 128;

#[derive(Default, Debug, Clone, Serialize)]
pub struct Info {
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
pub struct Summary {
    info: Info,
    vcids: Vec<(Vcid, Info)>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FrameType {
    AOS,
}

fn aos_block_len(length: usize, fec: bool, ocf: bool, izone: usize) -> usize {
    let mut block_len = length;
    block_len += izone;
    if fec {
        block_len += FEC_LEN;
    }
    if ocf {
        block_len += OCF_LEN;
    }
    return block_len + RS_PARITY_LEN;
}

pub fn frame_aos<O: AsRef<Path>>(
    input: InputReader,
    length: usize,
    pn: bool,
    fec: bool,
    ocf: bool,
    keep_fill: bool,
    izone: usize,
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
    let block_len = aos_block_len(length, fec, ocf, izone);
    let mut pipeline = Pipeline::new(block_len);
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
        pipeline = pipeline.with_rs(opts);
    }

    let mut summary = Summary {
        info: Info::default(),
        vcids: Vec::default(),
    };
    let mut vcids: HashMap<Vcid, Info> = HashMap::default();

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
        summary.info.total_frames += 1;
        summary.info.total_bytes += frame.data.len();

        let channel = vcids.entry(frame.header.vcid).or_default();
        channel.total_frames += 1;
        channel.total_bytes += frame.data.len();
        channel.missing_frames += frame.missing as usize;
        match &frame.integrity {
            Some(integrity) => match integrity {
                Integrity::Ok => {
                    channel.ok += 1;
                    summary.info.ok += 1;
                }
                Integrity::Corrected => {
                    channel.corrected += 1;
                    summary.info.corrected += 1;
                }
                Integrity::Uncorrectable | Integrity::NotCorrected => {
                    channel.uncorrectable += 1;
                    summary.info.uncorrectable += 1;
                }
                Integrity::Failed => {
                    channel.error += 1;
                    summary.info.error += 1;
                }
            },
            None => {
                channel.not_performed += 1;
                summary.info.not_performed += 1;
            }
        }
        match &frame.integrity {
            Some(Integrity::Uncorrectable | Integrity::NotCorrected | Integrity::Failed) => {
                warn!(vcid = %frame.header.vcid, counter = frame.header.counter, integrity = ?frame.integrity, "frame integrity failed, dropping");
                continue;
            }
            _ => {}
        }

        if let Some(mut fp) = dst.as_ref() {
            fp.write_all(&frame.data[..length])?;
        }
    }

    for (vcid, each) in vcids {
        summary.vcids.push((vcid, each));
    }

    Ok(summary)
}

pub fn render_summary(summary: &Summary, format: SummaryFormat) -> Result<()> {
    match format {
        SummaryFormat::JSON => serde_json::to_writer_pretty(stdout(), &summary)
            .map_err(|err| anyhow!("writing output: {err}")),
        _ => bail!("{:?} not currently supported", format),
    }
}
