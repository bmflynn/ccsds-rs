use std::fs;
use std::process;

use crate::TimecodeFormat;
use ccsds::spacepacket::*;

pub(crate) fn do_info(input: String, _gaps: bool, tc_fmt: &TimecodeFormat) -> process::ExitCode {
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
    let summary = summarize(&mut fp, tc_parser);

    println!("{}", serde_json::to_string(&summary).unwrap());

    return process::ExitCode::SUCCESS;
}
