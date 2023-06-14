use std::fs;
use std::process;

use crate::TimecodeFormat;
use ccsds::spacepacket::*;

pub(crate) fn do_inspect(input: String, tc_fmt: &TimecodeFormat) -> process::ExitCode {
    let mut fp = match fs::File::open(input) {
        Ok(f) => f,
        Err(e) => {
            println!("failed to open input: {}", e);
            return process::ExitCode::FAILURE;
        }
    };
    let tc_parser: &TimecodeParser = match tc_fmt {
        TimecodeFormat::CDS => &parse_cds_timecode,
        TimecodeFormat::EOSCUC => &parse_eoscuc_timecode,
    };
    let stream = Stream::new(&mut fp);
    let mut summarizer = Summarizer::new(&parse_cds_timecode);
    for packet in stream {
        summarizer.add(&packet);
    }

    let summary = summarizer.result();
    
    todo!();

    return process::ExitCode::SUCCESS;
}
