use std::{io::Write, path::PathBuf};

use anyhow::{Context, Result};
use ccsds::Apid;

pub fn merge<W, T>(
    inputs: &[PathBuf],
    time_decoder: &T,
    writer: W,
    order: Option<Vec<Apid>>,
) -> Result<()>
where
    T: ccsds::TimeDecoder,
    W: Write,
{
    ccsds::merge_by_timecode(inputs, time_decoder, writer, order)
        .with_context(|| format!("Merging {} inputs", inputs.len()))
}
