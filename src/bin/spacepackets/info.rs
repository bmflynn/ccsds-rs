use std::cmp;
use std::collections::HashMap;
use std::fs;
use std::process;

use crate::TimecodeFormat;
use ccsds::error::DecodeError;
use ccsds::spacepacket::*;
use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;

#[derive(Debug)]
struct ApidInfo {
    id: u16,
    total_count: u32,
    total_bytes: usize,
    gaps: HashMap<u16, Gap>,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
}

#[derive(Debug)]
struct Summary {
    apids: HashMap<u16, ApidInfo>,
    total_count: u32,
    total_bytes: usize,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
}

fn get_datetime(pkt: &Packet, fmt: TimecodeFormat) -> Result<DateTime<Utc>, DecodeError> {
    match fmt {
        TimecodeFormat::CDS => HasTimecode::<CDSTimecode>::timecode(pkt)?.timestamp(),
        TimecodeFormat::EOSCUC => HasTimecode::<EOSCUCTimecode>::timecode(pkt)?.timestamp(),
        _ => Err(DecodeError::Other(String::from(
            "unsupported timestamp format",
        ))),
    }
}

fn summarize(input: String, tc_fmt: TimecodeFormat) -> Result<Summary, DecodeError> {
    let mut f = fs::File::open(input)?;
    let mut summary = Summary {
        apids: HashMap::new(),
        total_count: 0,
        total_bytes: 0,
        first: Utc::now(),
        last: Utc.timestamp(0, 0),
    };

    let sequencer = Sequencer::new(Stream::new(&mut f));
    
    sequencer.for_each(|x| {
        let pkt = x.unwrap();
        let total_bytes = PrimaryHeader::SIZE + pkt.data.len();

        summary.total_count += 1;
        summary.total_bytes += total_bytes;

        let mut apid = match summary.apids.remove(&pkt.header.apid) {
            Some(a) => a,
            None => ApidInfo {
                id: pkt.header.apid,
                total_count: 1,
                total_bytes: total_bytes,
                gaps: HashMap::new(),
                first: Utc::now(),
                last: Utc.timestamp(0, 0),
            },
        };
        apid.total_count += 1;
        apid.total_bytes += total_bytes;

        if pkt.header.has_secondary_header
            && (pkt.header.sequence_flags == SEQ_FIRST || pkt.header.sequence_flags == SEQ_STANDALONE)
        {
            match get_datetime(&pkt, tc_fmt) {
                Ok(dt) => {
                    summary.first = cmp::min(summary.first, dt.clone());
                    summary.last = cmp::max(summary.last, dt.clone());
                    apid.first = cmp::min(apid.first, dt.clone());
                    apid.last = cmp::max(apid.last, dt.clone());
                }
                _ => {}
            };
        };

        summary.apids.insert(pkt.header.apid, apid);
    });
   
    sequencer.gaps();
    Ok(summary)
}

pub(crate) fn do_info(input: String, gaps: bool, tc_fmt: &TimecodeFormat) -> process::ExitCode {
    let summary = match summarize(input, *tc_fmt) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed: {}", e);
            return process::ExitCode::SUCCESS;
        }
    };

    println!("{:?}", summary);

    return process::ExitCode::SUCCESS;
}
