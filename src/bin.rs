use std::error::Error;
use std::fs::File;

use ccsds::stream::Stream;
use ccsds::stream::Sequencer;

fn main() -> Result<(), Box<dyn Error>> {
    let fp = File::open("snpp_cris.dat")?;
    let stream = Stream::new(Box::new(fp));
    let sequencer = Sequencer::new(stream);

    let packets = sequencer.filter(|zult| zult.is_ok()).map(|zult| zult.unwrap());

    for pkt in packets {
        println!("{:?} data_len:{}", pkt.header, pkt.data.len());
    }

    return Ok(());
}
