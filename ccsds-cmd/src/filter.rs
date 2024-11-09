use std::{
    collections::HashSet,
    io::{Read, Write},
};

use anyhow::{bail, Result};
use ccsds::{Apid, CdsTimeDecoder, TimeDecoder};
use hifitime::{Duration, Epoch};
use tracing::trace;

struct Ptr(Vec<u8>, Apid, Epoch);

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
            let nanos = time_decoder.decode_time(first).unwrap_or_else(|_| {
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

            Some(Ptr(data, apid, nanos))
        })
}

pub fn filter<R, W>(
    input: R,
    mut writer: W,
    include: &[Apid],
    exclude: &[Apid],
    before: Option<Epoch>,
    after: Option<Epoch>,
) -> Result<()>
where
    R: Read + Send,
    W: Write,
{
    let min_epoch = Epoch::from_utc_duration(Duration::from_days(0.0));
    let max_epoch = Epoch::from_utc_duration(Duration::from_days(73049.0));

    if include.is_empty() && exclude.is_empty() && before.is_none() && after.is_none() {
        bail!("no filters specified");
    }

    let packets: Box<dyn Iterator<Item = Ptr>> = if before.is_some() || after.is_some() {
        Box::new(packets_with_times(input))
    } else {
        Box::new(
            ccsds::read_packets(input)
                .map_while(Result::ok)
                .map(|p| Ptr(p.data, p.header.apid, min_epoch)),
        ) as Box<dyn Iterator<Item = Ptr>>
    };

    let including = !include.is_empty();
    let excluding = !exclude.is_empty();
    let include: HashSet<Apid> = include.iter().copied().collect();
    let exclude: HashSet<Apid> = exclude.iter().copied().collect();
    let have_before = before.is_some();
    let before = before.unwrap_or(max_epoch);
    let have_after = after.is_some();
    let after = after.unwrap_or(min_epoch);

    for Ptr(data, apid, stamp) in packets {
        if have_before && have_after && stamp < after || stamp >= before {
            trace!(
                apid,
                ?stamp,
                len = data.len(),
                ?before,
                ?after,
                "skip after/before"
            );
            continue;
        } else if have_before && stamp >= before {
            trace!(
                apid,
                ?stamp,
                len = data.len(),
                ?before,
                ?after,
                "skip before"
            );
            continue;
        } else if have_after && stamp < after {
            trace!(
                apid,
                ?stamp,
                len = data.len(),
                ?before,
                ?after,
                "skip after"
            );
            continue;
        }
        if including && excluding {
            if include.contains(&apid) && !exclude.contains(&apid) {
                writer.write_all(&data)?;
            } else {
                trace!(
                    apid,
                    ?stamp,
                    len = data.len(),
                    "skip included, not excluded"
                );
            }
        } else if (including && include.contains(&apid))
            || (excluding && !exclude.contains(&apid))
            || !(including || excluding)
        {
            writer.write_all(&data)?;
        }
    }

    Ok(())
}
