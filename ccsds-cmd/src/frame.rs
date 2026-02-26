use anyhow::{Context, Result};
use ccsds::{
    framing::{synchronize, Frame, Integrity, Pipeline, RsOpts, SyncOpts, Vcid, ASM},
    spacepacket::Apid,
};
use handlebars::handlebars_helper;
use serde::Serialize;
use spacecrafts::FramingConfig;
use std::{
    collections::HashMap,
    fs::File,
    io::{stdout, BufReader, Read, Write},
    path::Path,
};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub enum InputType {
    Cadu,
    Frame,
}

impl InputType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cadu" => Some(Self::Cadu),
            "frame" => Some(Self::Frame),
            _ => None,
        }
    }
}

pub fn sync(srcpath: &Path, dstpath: &Path, block_size: usize) -> Result<()> {
    let src = BufReader::new(File::open(srcpath).context("opening source")?);
    let mut dst = File::create(dstpath).context("creating dest")?;

    for cadu in synchronize(src, SyncOpts::new(block_size)).map_while(Result::ok) {
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
    correct: bool,
) -> Result<()> {
    let block_len = config.codeblock_len();
    let mut pipeline = Pipeline::new(block_len);

    if config.pseudo_noise.is_none() {
        pipeline = pipeline.without_derandomization();
    }

    if let Some(rs_config) = &config.reed_solomon {
        let opts = RsOpts::new(rs_config.interleave)
            .with_virtual_fill(rs_config.virtual_fill_length)
            .with_correction(correct)
            .with_detection(correct)
            .with_num_threads(0);
        pipeline = pipeline.with_rs(opts);
    }

    let frame_len = config.length;
    let src = BufReader::new(File::open(srcpath).context("opening source")?);
    let frames = pipeline.start(src);

    let mut dst = File::create(dstpath).context("creating dest")?;

    for frame in frames {
        if frame.is_fill() {
            continue;
        }
        if !include.is_empty() && !include.contains(&frame.header.vcid) {
            continue;
        }
        if !exclude.is_empty() && exclude.contains(&frame.header.vcid) {
            continue;
        }

        let mpdu = &frame
            .mpdu(config.insert_zone_length, config.trailer_length)
            .unwrap();

        debug!(
            "{:?} {:?} missing={} integrity={}",
            &frame.header,
            mpdu,
            &frame.missing,
            &frame
                .integrity
                .clone()
                .map_or_else(|| "None".to_string(), |i| format!("{i:?}"))
        );

        match &frame.integrity {
            Some(Integrity::Uncorrectable | Integrity::NotCorrected | Integrity::Failed) => {
                warn!(vcid = %frame.header.vcid, "frame integrity failed, dropping");
                continue;
            }
            _ => {}
        }
        // if frame.data.len() != frame_len {
        //     warn!(
        //         "expected frame length={frame_len} at frame idx {idx}, got {}; dropping",
        //         frame.data.len()
        //     );
        //     continue;
        // }

        dst.write_all(&frame.data[..frame_len])?;
    }

    Ok(())
}

pub fn packetize(
    srcpath: &Path,
    dstpath: &Path,
    config: FramingConfig,
    include: Vec<Apid>,
    exclude: Vec<Apid>,
) -> Result<()> {
    let mut src = BufReader::new(File::open(srcpath).context("opening source")?);

    let (frames_tx, frames_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; config.length];
        loop {
            if let Err(err) = src.read_exact(&mut buf) {
                debug!("failed to read frame: {err}; bailing!");
                break;
            }
            let Some(frame) = Frame::decode(buf.clone()) else {
                warn!("failed to decode frame, skipping");
                continue;
            };
            if frames_tx.send(frame).is_err() {
                warn!("failed to send frame; bailing!");
                break;
            }
        }
    });

    let mut dst = File::create(dstpath).context("creating dest")?;
    let frames = frames_rx.into_iter();
    let packets =
        ccsds::framing::packet_decoder(frames, config.insert_zone_length, config.trailer_length);
    for packet in packets {
        if !include.is_empty() && !include.contains(&packet.header.apid) {
            continue;
        }
        if !exclude.is_empty() && exclude.contains(&packet.header.apid) {
            continue;
        }
        if let Err(err) = dst.write_all(&packet.data) {
            warn!("failed to write packet: {err}");
            break;
        }
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

fn frames_from_cadu(config: FramingConfig, fpath: &Path) -> Result<impl Iterator<Item = Frame>> {
    let block_len = config.codeblock_len();
    let mut pipeline = Pipeline::new(block_len);

    if config.pseudo_noise.is_none() {
        pipeline = pipeline.without_derandomization();
    }

    if let Some(rs_config) = &config.reed_solomon {
        let opts = RsOpts::new(rs_config.interleave)
            .with_virtual_fill(rs_config.virtual_fill_length)
            .with_correction(true)
            .with_detection(true)
            .with_num_threads(0);
        pipeline = pipeline.with_rs(opts);
    }

    let src = BufReader::new(File::open(fpath).context("opening source")?);
    let frames = pipeline.start(src);

    Ok(frames)
}

struct FrameIter {
    file: BufReader<File>,
    frame_len: usize,
}

impl Iterator for FrameIter {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = vec![0u8; self.frame_len];
        match self.file.read_exact(&mut buf) {
            Ok(_) => Frame::decode(buf),
            Err(_) => None,
        }
    }
}

fn frames_from_file(config: FramingConfig, fpath: &Path) -> Result<impl Iterator<Item = Frame>> {
    let frame_len = config.length;
    let src = BufReader::new(File::open(fpath).context("opening source")?);
    Ok(FrameIter {
        file: src,
        frame_len,
    }
    .into_iter())
}

pub fn info(config: FramingConfig, fpath: &Path, format: &Format, itype: InputType) -> Result<()> {
    let frames =
        match itype {
            InputType::Cadu => Box::new(frames_from_cadu(config.clone(), fpath)?)
                as Box<dyn Iterator<Item = Frame>>,
            InputType::Frame => Box::new(frames_from_file(config.clone(), fpath)?)
                as Box<dyn Iterator<Item = Frame>>,
        };

    let mut info = Info {
        filename: fpath.file_name().unwrap().to_string_lossy().to_string(),
        summary: Summary::default(),
        vcids: Vec::default(),
    };
    let mut vcids: HashMap<Vcid, Summary> = HashMap::default();
    for frame in frames {
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
                Integrity::Uncorrectable | Integrity::NotCorrected => {
                    sum.uncorrectable += 1;
                    info.summary.uncorrectable += 1;
                }
                Integrity::Failed => {
                    sum.error += 1;
                    info.summary.error += 1;
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
