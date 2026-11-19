use anyhow::{Context, Result};
use ccsds::spacepacket::{
    decode_packets, missing_packets,
    timecode::{PacketApidTimeDecoder, PacketTimeDecoder},
    Apid, Packet,
};
use hifitime::{Duration, Epoch};
use serde::Serialize;
use std::{
    cmp,
    collections::HashMap,
    io::{stdout, Write},
    path::Path,
};
use tracing::{trace, warn};

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
    total_packets: usize,
    missing_packets: usize,
    /// Packets with the same sequence id as the packet before them in the same APID. These are
    /// included in `total_packets` and never counted in `missing_packets`.
    duplicate_packets: usize,
    first_packet_time: Option<Epoch>,
    last_packet_time: Option<Epoch>,
    duration: Duration,
}

#[derive(Debug, Clone, Serialize)]
struct Info {
    filename: String,
    summary: Summary,
    apids: Vec<(Apid, Summary)>,
}

fn summarize(fpath: &Path, timecodes: Option<PacketApidTimeDecoder>) -> Result<Info> {
    let reader = std::fs::File::open(fpath).context("opening input")?;
    let packets = decode_packets(reader).filter_map(|r| match r {
        Ok(packet) => Some(packet),
        Err(err) => {
            // A packet that fails to decode is not counted, so make sure it is visible.
            warn!(%err, "failed to decode packet, skipping");
            None
        }
    });
    let (summary, apids) = summarize_packets(packets, timecodes.as_ref());

    Ok(Info {
        filename: fpath.to_string_lossy().to_string(),
        summary,
        apids,
    })
}

fn summarize_packets(
    packets: impl Iterator<Item = Packet>,
    timecodes: Option<&PacketApidTimeDecoder>,
) -> (Summary, Vec<(Apid, Summary)>) {
    let mut last_seqid: HashMap<Apid, u16> = HashMap::default();
    let mut apids: HashMap<Apid, Summary> = HashMap::default();
    let mut summary = Summary::default();

    for packet in packets {
        summary.total_packets += 1;

        let cur = packet.header.sequence_id;
        // `missing_packets` returns 0 for a repeated sequence id, so check for it separately
        let (missing, duplicate) = match last_seqid.get(&packet.header.apid) {
            Some(last) => (missing_packets(cur, *last), cur == *last),
            None => (0, false), // first packet seen for this APID
        };
        last_seqid.insert(packet.header.apid, cur);
        summary.missing_packets += missing as usize;
        summary.duplicate_packets += usize::from(duplicate);

        let apid = apids.entry(packet.header.apid).or_default();
        apid.total_packets += 1;
        apid.missing_packets += missing as usize;
        apid.duplicate_packets += usize::from(duplicate);

        if !packet.header.has_secondary_header {
            continue;
        }

        // NOTE: The resulting Epoch will have a reference time of Jan 1, 1900 and must be
        // converted to the basetime for the specific APID.
        if let Some(time_decoder) = timecodes {
            match time_decoder.decode(&packet) {
                Ok(Some(epoch)) => {
                    summary.first_packet_time = summary
                        .first_packet_time
                        .map_or(Some(epoch), |cur| Some(cmp::min(epoch, cur)));
                    summary.last_packet_time = summary
                        .last_packet_time
                        .map_or(Some(epoch), |cur| Some(cmp::max(epoch, cur)));
                    if let (Some(first), Some(last)) =
                        (summary.first_packet_time, summary.last_packet_time)
                    {
                        summary.duration = last - first;
                    }

                    apid.first_packet_time = apid
                        .first_packet_time
                        .map_or(Some(epoch), |cur| Some(cmp::min(epoch, cur)));
                    apid.last_packet_time = apid
                        .last_packet_time
                        .map_or(Some(epoch), |cur| Some(cmp::max(epoch, cur)));
                    if let (Some(first), Some(last)) =
                        (apid.first_packet_time, apid.last_packet_time)
                    {
                        apid.duration = last - first;
                    }
                }
                Ok(None) => {
                    trace!(header=?packet.header, "no timecode config");
                }
                Err(err) => {
                    warn!(%err, "failed to decode timecode")
                }
            }
        }
    }

    let mut apids: Vec<(Apid, Summary)> = apids.into_iter().collect();
    apids.sort_unstable_by_key(|(k, _)| *k);

    (summary, apids)
}

pub fn info(
    fpath: &Path,
    format: &Format,
    time_decoder: Option<PacketApidTimeDecoder>,
) -> Result<()> {
    let info = summarize(fpath, time_decoder)?;

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
    let mut hb = crate::render::engine();
    assert!(hb.register_template_string("info", TEXT_TEMPLATE).is_ok());

    hb.render("info", &info).context("rendering text")
}

const TEXT_TEMPLATE: &str = r#"{{ filename }}
===============================================================================================
First:    {{ summary.first_packet_time }}
Last:     {{ summary.last_packet_time }} 
Duration: {{ summary.duration }}
APIDS:    {{ #each apids }}{{ this.[0] }}{{ #if @last }}{{ else }}, {{ /if }}{{ /each }}
Count:    {{ summary.total_packets }}
Missing:  {{ summary.missing_packets }}
Duplicate: {{ summary.duplicate_packets }}
-----------------------------------------------------------------------------------------------
APID    First                              Last                                 Count   Missing
-----------------------------------------------------------------------------------------------
{{ #each apids }}{{ lpad 6 this.[0] }}  {{ #with this.[1] }}{{ lpad 33 first_packet_time }}  {{ lpad 33 last_packet_time }}   {{ lpad 6 total_packets }}   {{ lpad 7 missing_packets }}{{ /with }}
{{/each }}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal packet with the given apid (low byte) and sequence id.
    fn packet(apid_lo: u8, seq: u16) -> Packet {
        #[rustfmt::skip]
        let dat = [
            0x0d, apid_lo, 0xc0 | (seq >> 8) as u8, (seq & 0xff) as u8, 0x0, 0x8,
            0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        Packet::decode(&dat).unwrap()
    }

    fn run(pkts: Vec<Packet>) -> (Summary, Vec<(Apid, Summary)>) {
        summarize_packets(pkts.into_iter(), None)
    }

    #[test]
    fn no_missing() {
        let (sum, apids) = run(vec![packet(0x59, 1), packet(0x59, 2), packet(0x59, 3)]);
        assert_eq!(sum.total_packets, 3);
        assert_eq!(sum.missing_packets, 0);
        assert_eq!(sum.duplicate_packets, 0);
        assert_eq!(apids.len(), 1);
    }

    #[test]
    fn counts_gap() {
        // 2 -> 5 is missing 3 and 4
        let (sum, apids) = run(vec![packet(0x59, 1), packet(0x59, 2), packet(0x59, 5)]);
        assert_eq!(sum.total_packets, 3);
        assert_eq!(sum.missing_packets, 2);
        assert_eq!(apids[0].1.missing_packets, 2);
    }

    #[test]
    fn sequence_wrap_is_not_missing() {
        let (sum, _) = run(vec![
            packet(0x59, 16382),
            packet(0x59, 16383),
            packet(0x59, 0),
        ]);
        assert_eq!(sum.missing_packets, 0);
    }

    #[test]
    fn counts_gap_across_wrap() {
        // 16382 -> 1 is missing 16383 and 0
        let (sum, _) = run(vec![packet(0x59, 16382), packet(0x59, 1)]);
        assert_eq!(sum.missing_packets, 2);
    }

    #[test]
    fn duplicate_is_not_missing() {
        let (sum, apids) = run(vec![packet(0x59, 1), packet(0x59, 1), packet(0x59, 2)]);
        assert_eq!(sum.total_packets, 3);
        assert_eq!(sum.missing_packets, 0);
        assert_eq!(sum.duplicate_packets, 1);
        assert_eq!(apids[0].1.duplicate_packets, 1);
    }

    #[test]
    fn apids_tracked_independently() {
        // Interleaved apids, each sequential on its own. apid 0x5a has one gap.
        let (sum, apids) = run(vec![
            packet(0x59, 1),
            packet(0x5a, 10),
            packet(0x59, 2),
            packet(0x5a, 12),
        ]);
        assert_eq!(sum.total_packets, 4);
        assert_eq!(sum.missing_packets, 1);
        assert_eq!(apids.len(), 2);
        assert_eq!(apids[0].1.missing_packets, 0);
        assert_eq!(apids[1].1.missing_packets, 1);
    }

    #[test]
    fn totals_match_sum_of_apids() {
        let (sum, apids) = run(vec![
            packet(0x59, 1),
            packet(0x59, 4),
            packet(0x5a, 7),
            packet(0x5a, 7),
            packet(0x5a, 8),
        ]);
        let total: usize = apids.iter().map(|(_, s)| s.total_packets).sum();
        let missing: usize = apids.iter().map(|(_, s)| s.missing_packets).sum();
        let dups: usize = apids.iter().map(|(_, s)| s.duplicate_packets).sum();
        assert_eq!(sum.total_packets, total);
        assert_eq!(sum.missing_packets, missing);
        assert_eq!(sum.duplicate_packets, dups);
    }
}
