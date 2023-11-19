mod bytes;
mod pn;
mod rs;
mod synchronizer;

use std::io::Read;
use std::thread;
use std::sync::mpsc::{sync_channel, Receiver, channel};

pub use pn::decode as pn_decode;
pub use rs::{
    correct_codeblock as rs_correct_codeblock, correct_message as rs_correct_message,
    deinterlace as rs_deinterlace, has_errors as rs_has_errors, RSState,
};
use serde::{Deserialize, Serialize};
pub use synchronizer::{BlockIter, Synchronizer, ASM};

pub const TERRA: i32 = 42;
pub const AQUA: i32 = 154;
pub const SNPP: i32 = 157;
pub const NOAA20: i32 = 159;
pub const NOAA21: i32 = 177;
pub const NOAA22: i32 = 178;
pub const NOAA23: i32 = 179;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RSConfig {
    pub interleave: i32,
    pub correctable: i32,
    pub vfill_length: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Framing {
    pub asm: Vec<u8>,
    /// Length of the frame contained within a CADU, not including the ASM or
    /// any Reed-solomon parity bytes.
    pub frame_length: i32,
    pub pseudo_randomized: bool,
    pub izone_length: i32,
    pub trailer_length: i32,
    pub rs: Option<RSConfig>,
}

impl Framing {
    /// Returns the expected length of CADU which will include the ASM and the
    /// length of the Reed-Solomon code block.
    ///
    /// So, for example, with standard RS(223/255) with an interleave of 4 this
    /// will return 1024, which is 4 bytes for the ASM, 128 bits for the Reed-Solomon
    /// code block and the frame bytes.
    pub fn cadu_len(&self) -> i32 {
        let rslen = match self.rs {
            Some(ref rs) => (rs.correctable * 2) * rs.interleave,
            None => 0,
        };
        self.asm.len() as i32 + self.frame_length + rslen
    }
}

/// Get spacecraft framing info for a particular spacecraft.
pub fn get_framing(scid: i32) -> Option<Framing> {
    match scid {
        TERRA | AQUA | SNPP | NOAA20 => Some(Framing {
            asm: ASM.to_vec(),
            frame_length: 892,
            pseudo_randomized: true,
            izone_length: 0,
            trailer_length: 0,
            rs: Some(RSConfig {
                interleave: 4,
                correctable: 16,
                vfill_length: 0,
            }),
        }),
        NOAA21 | NOAA22 | NOAA23 => Some(Framing {
            asm: ASM.to_vec(),
            frame_length: 1115,
            pseudo_randomized: true,
            izone_length: 0,
            trailer_length: 0,
            rs: Some(RSConfig {
                interleave: 4,
                correctable: 16,
                vfill_length: 0,
            }),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_cadu_len() {
        let framing = get_framing(157).unwrap();
        assert_eq!(framing.cadu_len(), 1024);
    }
}

pub const VCID_FILL: u16 = 63;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct VCDUHeader {
    pub version: u8,
    pub scid: u16,
    pub vcid: u16,
    pub counter: u32,
    pub replay: bool,
    pub cycle: bool,
    pub counter_cycle: u8,
}

impl VCDUHeader {
    const LEN: u32 = 6;

    pub fn decode(dat: &Vec<u8>) -> Self {
        if dat.len() < Self::LEN as usize {
            panic!(
                "vcdu header requires {} bytes, got {}",
                Self::LEN,
                dat.len()
            );
        }
        let x = u16::from_be_bytes([dat[0], dat[1]]);
        VCDUHeader {
            version: (dat[0] >> 6) & 0x3,
            scid: (x >> 6) & 0xff,
            vcid: x & 0x3f,
            counter: u32::from_be_bytes([0, dat[2], dat[3], dat[4]]),
            replay: (dat[5] >> 7) & 0x1 == 1,
            cycle: (dat[5] >> 6) & 0x1 == 1,
            counter_cycle: dat[5] & 0xf,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn decode_vcduheader() {
        let dat: Vec<u8> = vec![
            0x55, 0x61, // version 1, scid 85, vcid 33
            0x01, 0xe2, 0x40, // counter 123456
            0x05, // replay:false, frame count usage:false, frame-count-cycle:5
            0x01, 0x02, 0x03, // insert zone
            0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0xaa, // first-header-pointer 682
        ];

        let header = VCDUHeader::decode(&dat);

        assert_eq!(header.version, 1);
        assert_eq!(header.scid, 85);
        assert_eq!(header.vcid, 33);
        assert_eq!(header.counter, 123456);
        assert!(!header.replay);
        assert!(!header.cycle);
        assert_eq!(header.counter_cycle, 5);
    }

    #[test]
    fn decode_vcduheader_panics_when_data_too_short() {
        let zult = std::panic::catch_unwind(|| VCDUHeader::decode(&vec![0u8; 0]));
        assert!(zult.is_err(), "decode should panic with too little data");
    }

    #[test]
    fn decode_frame() {
        let dat: Vec<u8> = vec![
            0x55, 0x61, // version 1, scid 85, vcid 33
            0x01, 0xe2, 0x40, // counter 123456
            0x05, // replay:false, frame count usage:false, frame-count-cycle:5
            0x00, 0x00, 0x00,
        ];
        let frame = Frame::decode(dat);

        assert_eq!(frame.data.len(), 3);
    }
}

#[derive(Debug)]
pub struct Frame {
    pub header: VCDUHeader,
    pub data: Vec<u8>,
}

impl Frame {
    pub fn decode(dat: Vec<u8>) -> Self {
        let header = VCDUHeader::decode(&dat);
        Frame {
            header,
            data: dat[VCDUHeader::LEN as usize..].to_vec(),
        }
    }
}

pub struct DecodedFrameIter {
    jobs: Receiver<Receiver<(Frame, RSState)>>,
}

impl Iterator for DecodedFrameIter {
    type Item = (Frame, RSState);

    fn next(&mut self) -> Option<Self::Item> {
        return match self.jobs.recv() {
            Err(_) => None,
            Ok(rx) => Some(rx.recv().expect("failed to receive future")),
        }
    }
}

/// Return a [DecodedFrameIter] that will return all frames decoded from [reader].
///
/// IO is performed concurrently so the iterator can be returned immediately. All PN 
/// and RS decoding is likewise performed concurrently.
///
/// Frames will generated in the order in which they occur in the original byte stream.
pub fn decoded_frames_iter(
    reader: impl Read + Send + 'static,
    cadu_length: i32,
    interleave: i32,
) -> impl Iterator<Item = (Frame, RSState)> {
    // Bounded channel on which to recieve 
    // be received.
    let (jobs_tx, jobs_rx) = sync_channel(1024);

    // Do IO (Read/synchronize) in the background where each synchronized block or 
    // CADU will be submitted to a thread pool such that the PN and RS can run in the 
    // background. 
    thread::spawn(move || {
        let jobs_tx = jobs_tx.clone();
        // let reader = File::open(source.to_owned()).unwrap();
        let asm = ASM.to_vec();
        let synchronizer = Synchronizer::new(reader, &asm, cadu_length - asm.len() as i32);

        for block in synchronizer {
            if let Err(_) = block {
                continue;
            }
            let mut block = block.unwrap();

            let (future_tx, future_rx) = channel();
            rayon::spawn_fifo(move || {
                pn_decode(&mut block);
                let (dat, state) = rs_correct_codeblock(block.to_vec(), interleave);
                let frame = Frame::decode(dat);
                future_tx.send((frame, state)).expect("failed to send frame");
            });
            jobs_tx.send(future_rx).expect("failed to send future receiver");
        }
    });

    Box::new(DecodedFrameIter { jobs: jobs_rx })
}
