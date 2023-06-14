use std::process;

use crate::TimecodeFormat;
use ccsds::spacepacket::APID;
use chrono::Utc;

pub(crate) fn do_edit(
    input: String,
    apids: Vec<APID>,
    start: Option<String>,
    end: Option<String>,
    tc_fmt: &TimecodeFormat,
) -> process::ExitCode {
    println!("input: {:?}", input);
    println!("tc_fmt: {:?}", tc_fmt);
    println!("apids: {:?}", apids);
    todo!()
}
