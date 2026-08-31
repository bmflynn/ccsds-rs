use std::{
    collections::HashSet,
    io::{Read, Write},
};

use anyhow::{bail, Result};
use ccsds::spacepacket::{
    collect_groups, decode_packets,
    timecode::{PacketApidTimeDecoder, PacketTimeDecoder},
    Apid, PrimaryHeader,
};
use hifitime::{Duration, Epoch};
use tracing::{debug, trace};

struct Ptr(Vec<u8>, Apid, Epoch);

fn packets_with_times<R: Read + Send>(
    input: R,
    time_decoder: Option<PacketApidTimeDecoder>,
) -> impl Iterator<Item = Ptr> {
    let packets = decode_packets(input).filter_map(Result::ok);
    collect_groups(packets)
        .filter_map(Result::ok)
        .filter_map(move |g| {
            if g.packets.is_empty() || g.packets[0].is_last() || g.packets[0].is_cont() {
                // Drop incomplete packet groups
                return None;
            }
            // now we can be sure first packet has a timecode
            let first = &g.packets[0];
            let apid = first.header.apid;
            let nanos = match time_decoder.as_ref() {
                Some(tc) => match tc.decode(first) {
                    Ok(e) => match e {
                        Some(e) => e,
                        None => Epoch::from_unix_seconds(0.0),
                    },
                    Err(err) => {
                        debug!("failed to convert timecode to epoch: {err}");
                        return None;
                    }
                },
                None => Epoch::from_unix_seconds(0.0),
            };

            // total size of all packets in group
            let total_size = g
                .packets
                .iter()
                .map(|p| PrimaryHeader::LEN + p.header.len_minus1 as usize + 1)
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
    time_decoder: Option<PacketApidTimeDecoder>,
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
        Box::new(packets_with_times(input, time_decoder))
    } else {
        Box::new(
            decode_packets(input)
                .map_while(Result::ok)
                .map(|p| Ptr(p.data, p.header.apid, min_epoch)),
        ) as Box<dyn Iterator<Item = Ptr>>
    };

    let including = !include.is_empty();
    let include: HashSet<Apid> = include.iter().copied().collect();
    let excluding = !exclude.is_empty();
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
        if including && !include.contains(&apid) {
            trace!(apid, ?stamp, len = data.len(), "skip not included");
            continue;
        }
        if excluding && exclude.contains(&apid) {
            trace!(apid, ?stamp, len = data.len(), "skip excluded");
            continue;
        }
        writer.write_all(&data)?;
    }

    Ok(())
}
