use std::{io::Write, path::PathBuf};

use anyhow::{bail, Context, Result};
use ccsds::spacepacket::{Apid, Merger, TimecodeDecoder};
use hifitime::Epoch;

pub fn apid_order(name: &str) -> Option<Vec<Apid>> {
    if name == "jpss-viirs" {
        Some(vec![826, 821])
    } else {
        None
    }
}

pub fn merge<W>(
    inputs: &[PathBuf],
    time_decoder: TimecodeDecoder,
    writer: W,
    order: Option<Vec<Apid>>,
    from: Option<Epoch>,
    to: Option<Epoch>,
    apids: Option<&[Apid]>,
) -> Result<()>
where
    W: Write,
{
    if inputs.is_empty() {
        bail!("no inputs provided");
    }

    let from = from.map(|dt| (dt.to_utc_seconds() * 1_000_000.0) as u64);
    let to = to.map(|dt| (dt.to_utc_seconds() * 1_000_000.0) as u64);

    let mut merger = Merger::new(inputs.to_vec(), time_decoder);
    if let Some(order) = order {
        merger = merger.with_apid_order(&order);
    }
    if let Some(from) = from {
        merger = merger.with_from(from);
    }
    if let Some(to) = to {
        merger = merger.with_to(to);
    }
    if let Some(apids) = apids {
        merger = merger.with_apids(&apids);
    }

    merger
        .merge(writer)
        .with_context(|| format!("Merging {} inputs", inputs.len()))
}
