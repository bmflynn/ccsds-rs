use std::{
    fs::File,
    io::{stdout, BufReader, Read, Write},
    path::Path,
};

use anyhow::{anyhow, Result};
use spacecrafts::Spacecraft;
use tracing::debug;

use ccsds::framing::{Frame, Integrity, PacketDemux};

pub fn demux_packets<I: AsRef<Path>, O: AsRef<Path>>(
    input: I,
    sc: Spacecraft,
    output: Option<O>,
) -> Result<()> {
    let reader = File::open(input)?;
    let mut output: Box<dyn Write> = match output {
        Some(fpath) => Box::new(File::create(fpath)?),
        None => Box::new(stdout()),
    };
    debug!(cfg = ?sc.framing, "global framing config");
    let frame_len = sc.framing.length;
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

    let demux = {
        let mut d = PacketDemux::new(frames).with_defaults(
            sc.framing.izone_length,
            sc.framing.fhec_present,
            sc.framing.ocf_present,
            sc.framing.fec_present,
        );
        for ch in sc.vcids {
            let Some(cfg) = ch.framing else { continue };
            debug!(vcid = ch.vcid, ?cfg, "adding channel config");
            d = d.with_channel_config(
                ch.vcid,
                cfg.izone_length,
                cfg.fhec_present,
                cfg.ocf_present,
                cfg.fec_present,
            );
        }
        d
    };

    for packet in demux.into_iter() {
        output.write_all(&packet.data)?;
    }

    Ok(())
}

/// Iterate chunks of data from a buffererd reader
pub struct IterChunks {
    pub file: BufReader<File>,
    pub size: usize,
}

impl Iterator for IterChunks {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut num_read = 0usize;
        let mut buffer = vec![0u8; self.size];
        while num_read < self.size {
            match self.file.read(&mut buffer[num_read..]) {
                Ok(n) => {
                    if n == 0 {
                        return None;
                    }
                    num_read += n;
                }
                Err(e) => {
                    return Some(Err(anyhow!("reading: {e}")));
                }
            }
        }
        Some(Ok(buffer))
    }
}
