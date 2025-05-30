//
// Transition:
//    Init ->

use std::io::Read;

use crate::framing::{
    synchronizer::{Block, Loc},
    Frame,
};

use super::{derandomize, frame_decoder, reed_solomon, synchronize};

#[derive(Debug, Default)]
pub struct Pipeline {
    sync: bool,
    pn: bool,
    rs: Option<(u8, usize)>,
}

impl Pipeline {
    pub fn with_sync(mut self) -> Self {
        self.sync = true;
        self
    }
    pub fn with_default_pn(mut self) -> Self {
        self.pn = true;
        self
    }

    pub fn with_default_rs(mut self, interleave: u8, virtual_fill: usize) -> Self {
        self.rs = Some((interleave, virtual_fill));
        self
    }

    pub fn start<R: Read + Send + 'static>(
        self,
        reader: R,
        block_length: usize,
    ) -> impl Iterator<Item = Frame> {
        let mut blocks: Box<dyn Iterator<Item = Block> + Send + 'static> =
            blocks_iter(self.sync, reader, block_length);
        if self.pn {
            blocks = Box::new(derandomize(blocks))
        }

        let mut frames: Box<dyn Iterator<Item = Frame> + Send + 'static> =
            Box::new(frame_decoder(blocks));

        if let Some((interleave, virtual_fill)) = self.rs {
            frames = Box::new(reed_solomon(frames, interleave, virtual_fill));
        }

        frames
    }
}

fn blocks_iter<R: Read + Send + 'static>(
    sync: bool,
    reader: R,
    block_length: usize,
) -> Box<dyn Iterator<Item = Block> + Send + 'static> {
    if sync {
        Box::new(synchronize(reader, block_length))
    } else {
        Box::new(BlockReader::new(reader, block_length))
    }
}

struct BlockReader<R: Read> {
    reader: R,
    offset: usize,
    block_length: usize,
}

impl<R> BlockReader<R>
where
    R: Read,
{
    fn new(reader: R, block_length: usize) -> Self {
        BlockReader {
            reader,
            offset: 0,
            block_length,
        }
    }
}

impl<R> Iterator for BlockReader<R>
where
    R: Read,
{
    type Item = Block;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = vec![0u8; self.block_length];

        if self.reader.read(&mut buf).is_err() {
            return None;
        }
        let last = self.offset;
        self.offset += self.block_length;

        Some(Block {
            last,
            loc: Loc {
                offset: self.offset,
                bit: 0,
            },
            data: buf,
        })
    }
}
