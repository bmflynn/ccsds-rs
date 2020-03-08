use std::error::Error;
use std::fs::File;

use ccsds::stream::Stream;
use ccsds::timecode::{CDSTimecode, HasTimecode, Timecode};

fn main() -> Result<(), Box<dyn Error>> {
    let fp = File::open("input.dat")?;
    let stream = Stream::new(Box::new(fp));

    let packets = stream
        .filter(|zult| zult.is_ok())
        .map(|zult| zult.unwrap());

    for pkt in packets {
        // let tc: EOSCUCTimecode = pkt.timecode().unwrap();
        let tc: CDSTimecode = pkt.timecode().unwrap();
        // println!("{:?} {:?}", pkt.header, tc.timestamp());
        println!("{:?}", tc.timestamp());
    }

    return Ok(());
}
