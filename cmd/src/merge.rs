use std::{io::Write, path::PathBuf};

use anyhow::{bail, Context, Result};
use ccsds::Apid;
use chrono::{DateTime, FixedOffset};

pub fn merge<W, T>(
    inputs: &[PathBuf],
    time_decoder: &T,
    writer: W,
    order: Option<Vec<Apid>>,
    from: Option<DateTime<FixedOffset>>,
    to: Option<DateTime<FixedOffset>>,
    apids: Option<&[Apid]>,
) -> Result<()>
where
    T: ccsds::TimeDecoder,
    W: Write,
{
    if inputs.is_empty() {
        bail!("no inputs provided");
    }

    let from = from.map(|dt| dt.timestamp_micros() as u64);
    let to = to.map(|dt| dt.timestamp_micros() as u64);

    ccsds::merge_by_timecode(inputs, time_decoder, writer, order, from, to, apids)
        .with_context(|| format!("Merging {} inputs", inputs.len()))
}
