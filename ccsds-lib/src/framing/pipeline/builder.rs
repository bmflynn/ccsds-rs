use std::io::Read;

use crate::framing::{synchronizer::Block, Frame};

use super::{derandomize, frame_decoder, reed_solomon, synchronize, RsOpts, SyncOpts};

/// Builder class for constructing a typical CCSDS standard decode process.
#[derive(Debug)]
pub struct Pipeline {
    derandomize: bool,
    rs: Option<RsOpts>,
    block_length: usize,
}

impl Pipeline {
    /// Create a new pipeline with the specified Cadu length.
    ///
    /// Cadu length should be the total length of a Cadu in bytes minus the length of the attached
    /// sync marker being used (typically 4, see [crate::framing::ASM].
    pub fn new(cadu_length: usize) -> Self {
        Pipeline {
            derandomize: true,
            rs: None,
            block_length: cadu_length,
        }
    }

    pub fn without_derandomization(mut self) -> Self {
        self.derandomize = false;
        self
    }

    pub fn with_rs(mut self, opts: RsOpts) -> Self {
        self.rs = Some(opts);
        return self;
    }

    pub fn start<R: Read + Send + 'static>(&mut self, reader: R) -> impl Iterator<Item = Frame> {
        let mut blocks: Box<dyn Iterator<Item = Block> + Send + 'static> =
            Box::new(synchronize(reader, SyncOpts::new(self.block_length)).filter_map(Result::ok));

        if self.derandomize {
            blocks = Box::new(derandomize(blocks))
        }

        let mut frames: Box<dyn Iterator<Item = Frame> + Send + 'static> =
            Box::new(frame_decoder(blocks));

        if let Some(opts) = self.rs {
            let rs_frames = reed_solomon(frames, opts);
            frames = Box::new(rs_frames);
        }

        frames
    }
}
