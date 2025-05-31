use std::io::Read;

use tracing::debug;

use crate::framing::{
    synchronizer::{Block, Loc},
    Frame,
};

use super::{derandomize, frame_decoder, reed_solomon, synchronize, RsOpts};

#[derive(Debug)]
pub struct Pipeline {
    sync: bool,
    pn: bool,
    rs: Option<RsOpts>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline {
            sync: true,
            pn: true,
            rs: None,
            handles: Vec::default(),
        }
    }
    pub fn without_sync(mut self) -> Self {
        self.sync = false;
        self
    }
    pub fn without_pn(mut self) -> Self {
        self.pn = true;
        self
    }

    pub fn with_rs(mut self, opts: RsOpts) -> Self {
        self.rs = Some(opts);
        self
    }

    pub fn start<R: Read + Send + 'static>(
        &mut self,
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

        if let Some(opts) = self.rs {
            let (handle, rs_frames) = reed_solomon(frames, opts);
            self.handles.push(handle);
            frames = Box::new(rs_frames);
        }

        frames
    }

    pub fn shutdown(self) {
        for handle in self.handles {
            debug!("waiting for thread");
            handle
                .join()
                .unwrap_or_else(|err| panic!("reed_solomon thread paniced: {err:?}"));
        }
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
