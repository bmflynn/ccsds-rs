use std::{io::Write, path::PathBuf};

use anyhow::{Context, Result};

pub fn merge<W, T>(inputs: &[PathBuf], time_decoder: &T, writer: W) -> Result<()>
where
    T: ccsds::TimeDecoder,
    W: Write,
{
    ccsds::merge_by_timecode(inputs, time_decoder, writer)
        .with_context(|| format!("Merging {} inputs", inputs.len()))
}
