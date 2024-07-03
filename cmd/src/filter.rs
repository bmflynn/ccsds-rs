use std::{
    collections::HashSet,
    io::{Read, Write},
};

use anyhow::{bail, Result};
use ccsds::Apid;

pub fn filter<R, W>(input: R, mut writer: W, include: &[Apid], exclude: &[Apid]) -> Result<()>
where
    R: Read + Send,
    W: Write,
{
    if include.is_empty() && exclude.is_empty() {
        bail!("no filters specified");
    }

    let including = !include.is_empty();
    let excluding = !exclude.is_empty();
    let include: HashSet<Apid> = include.iter().copied().collect();
    let exclude: HashSet<Apid> = exclude.iter().copied().collect();
    for packet in ccsds::read_packets(input).map_while(Result::ok) {
        let apid = packet.header.apid;
        if including && excluding {
            if include.contains(&apid) && !exclude.contains(&apid) {
                writer.write_all(&packet.data)?;
            }
        } else if (including && include.contains(&apid)) || (excluding && !exclude.contains(&apid))
        {
            writer.write_all(&packet.data)?;
        }
    }

    Ok(())
}
