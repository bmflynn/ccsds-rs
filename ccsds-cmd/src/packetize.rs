use std::{
    fs::File,
    io::{stdout, BufReader, Read, Write},
    path::PathBuf,
};

use anyhow::{anyhow, Result};
use ccsds::framing::{packet_decoder, Frame, Integrity};

pub fn packetize(
    input: PathBuf,
    frame_len: usize,
    izone_length: usize,
    trailer_length: usize,
    output: Option<PathBuf>,
) -> Result<()> {
    let reader = File::open(input)?;
    let mut output: Box<dyn Write> = match output {
        Some(fpath) => Box::new(File::create(fpath)?),
        None => Box::new(stdout()),
    };

    let chunks = IterChunks {
        file: BufReader::new(reader),
        size: frame_len,
    };
    let frames = chunks
        .into_iter()
        .map_while(Result::ok)
        .filter_map(|chunk| Frame::decode(chunk))
        .filter(|f| !f.is_fill())
        .filter(|f| f.integrity != Some(Integrity::Uncorrectable));

    for packet in packet_decoder(frames, izone_length, trailer_length) {
        output.write_all(&packet.data)?;
    }

    Ok(())
}

struct IterChunks {
    file: BufReader<File>,
    size: usize,
}

impl Iterator for IterChunks {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buffer = vec![0u8; self.size];
        match self.file.read(&mut buffer) {
            Ok(n) => {
                if n < self.size {
                    // This discards partial reads
                    return None;
                }
                return Some(Ok(buffer));
            }
            Err(e) => Some(Err(anyhow!("reading: {e}"))),
        }
    }
}
