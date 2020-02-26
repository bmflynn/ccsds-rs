use std::error::Error;
use std::fs::File;

use ccsds::stream::Sequencer;
use ccsds::stream::Stream;
use ccsds::timecode::{EOSCUCTimecode, Timecode, HasTimecode};

fn main() -> Result<(), Box<dyn Error>> {
    let fp = File::open("input.dat")?;
    let stream = Stream::new(Box::new(fp));
    let sequencer = Sequencer::new(stream);

    let packets = sequencer
        .filter(|zult| zult.is_ok())
        .map(|zult| zult.unwrap());

    for pkt in packets {
        let tc: EOSCUCTimecode = pkt.timecode().unwrap();
        println!("{:?}", tc.timestamp());
    }

    return Ok(());
}
