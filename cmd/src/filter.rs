use std::{
    collections::HashSet,
    io::{Read, Write},
};

use anyhow::{bail, Result};
use ccsds::{Apid, CdsTimeDecoder, TimeDecoder};
use chrono::{DateTime, FixedOffset};

struct Ptr(Vec<u8>, Apid, u64);

fn packets_with_times<R: Read + Send>(input: R) -> impl Iterator<Item = Ptr> {
    ccsds::read_packet_groups(input)
        .filter_map(Result::ok)
        .filter_map(|g| {
            let time_decoder = &CdsTimeDecoder::default();

            if g.packets.is_empty() || !(g.packets[0].is_first() || g.packets[0].is_standalone()) {
                // Drop incomplete packet groups
                return None;
            }
            let first = &g.packets[0];
            let apid = first.header.apid;
            let usecs = time_decoder.decode_time(first).unwrap_or_else(|_| {
                panic!(
                    "failed to decode timecode from {first}: {:?}",
                    &first.data[..14]
                )
            });

            // total size of all packets in group
            let total_size = g
                .packets
                .iter()
                .map(|p| ccsds::PrimaryHeader::LEN + p.header.len_minus1 as usize + 1)
                .sum();

            let mut data = Vec::with_capacity(total_size);
            for packet in g.packets {
                data.extend(packet.data);
            }

            Some(Ptr(data, apid, usecs))
        })
}

pub fn filter<R, W>(
    input: R,
    mut writer: W,
    include: &[Apid],
    exclude: &[Apid],
    before: Option<DateTime<FixedOffset>>,
    after: Option<DateTime<FixedOffset>>,
) -> Result<()>
where
    R: Read + Send,
    W: Write,
{
    if include.is_empty() && exclude.is_empty() && before.is_none() && after.is_none() {
        bail!("no filters specified");
    }

    let packets: Box<dyn Iterator<Item = Ptr>> = if before.is_some() || after.is_some() {
        Box::new(packets_with_times(input))
    } else {
        Box::new(
            ccsds::read_packets(input)
                .map_while(Result::ok)
                .map(|p| Ptr(p.data, p.header.apid, 0)),
        ) as Box<dyn Iterator<Item = Ptr>>
    };

    let including = !include.is_empty();
    let excluding = !exclude.is_empty();
    let include: HashSet<Apid> = include.iter().copied().collect();
    let exclude: HashSet<Apid> = exclude.iter().copied().collect();
    let before_us = before.map_or(0, |dt| {
        u64::try_from(dt.timestamp_nanos_opt().unwrap()).unwrap() / 1000u64
    });
    let after_us = after.map_or(0, |dt| {
        u64::try_from(dt.timestamp_nanos_opt().unwrap()).unwrap() / 1000u64
    });

    for Ptr(data, apid, usecs) in packets {
        if before.is_some() && after.is_some() && (usecs < before_us || usecs > after_us) {
            continue;
        }
        if before.is_some() && usecs < before_us {
            continue;
        }
        if after.is_some() && usecs > after_us {
            continue;
        }
        if including && excluding {
            if include.contains(&apid) && !exclude.contains(&apid) {
                writer.write_all(&data)?;
            }
        } else if (including && include.contains(&apid)) || (excluding && !exclude.contains(&apid))
        {
            writer.write_all(&data)?;
        } else if !excluding && !including {
            writer.write_all(&data)?;
        }
    }

    Ok(())
}
