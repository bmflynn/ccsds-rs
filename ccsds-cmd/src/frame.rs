use anyhow::{bail, Context, Result};
use ccsds::{
    framing::{DefaultDerandomizer, DefaultReedSolomon, FrameDecoder, Synchronizer, ASM},
    prelude::Vcid,
};
use spacecrafts::FramingConfig;
use std::{
    fs::File,
    io::{BufReader, Write},
    path::Path,
};
use tracing::{debug, warn};

pub fn sync(srcpath: &Path, dstpath: &Path, block_size: usize) -> Result<()> {
    let src = BufReader::new(File::open(srcpath).context("opening source")?);
    let mut dst = File::create(dstpath).context("creating dest")?;

    let sync = Synchronizer::new(src, &ASM, block_size);

    for block in sync.into_iter().filter_map(Result::ok) {
        dst.write_all(&ASM)?;
        dst.write_all(&block)?;
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
    let src = BufReader::new(File::open(srcpath).context("opening source")?);
    let mut dst = File::create(dstpath).context("creating dest")?;

    let sync = Synchronizer::new(src, &ASM, config.codeblock_len());

    let mut framer = FrameDecoder::new();
    if config.pseudo_noise.is_some() {
        framer = framer.with_derandomization(Box::new(DefaultDerandomizer))
    }
    if let Some(rs_config) = config.reed_solomon {
        let rs = DefaultReedSolomon::new(rs_config.interleave);
        framer = framer.with_integrity(Box::new(rs));
    }

    let frame_len = config.length;

    for (idx, frame) in framer
        .decode(sync.into_iter().filter_map(Result::ok))
        .enumerate()
    {
        let frame = match frame {
            Ok(f) => f,
            Err(err) => bail!("frame decode failed: {err}"),
        };
        if !include.is_empty() && !include.contains(&frame.frame.header.vcid) {
            continue;
        }
        if !exclude.is_empty() && exclude.contains(&frame.frame.header.vcid) {
            continue;
        }

        let mpdu = frame
            .frame
            .mpdu(config.insert_zone_length, config.trailer_length)
            .unwrap();

        debug!(
            "{:?} {:?} missing={} integrity={}",
            frame.frame.header,
            mpdu,
            frame.missing,
            frame
                .integrity
                .map_or_else(|| "None".to_string(), |i| format!("{i:?}"))
        );

        if frame.frame.data.len() != frame_len {
            warn!(
                "expected frame length={frame_len} at frame idx {idx}, got {}; dropping",
                frame.frame.data.len()
            );
            continue;
        }

        dst.write_all(&frame.frame.data)?;
    }

    Ok(())
}
