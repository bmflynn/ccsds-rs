use std::io::Read;

use tracing::debug;

use crate::framing::{synchronizer::Synchronizer, ASM};

use super::Cadu;

#[allow(unused)]
fn sync_on_thread<R>(
    reader: R,
    block_length: usize,
    buffer_size: usize,
) -> impl Iterator<Item = Cadu>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = crossbeam::channel::bounded(buffer_size);

    std::thread::Builder::new()
        .name("synchronize".into())
        .spawn(move || {
            let sync = Synchronizer::new(reader, block_length);

            for zult in sync.into_iter() {
                match zult {
                    Ok(block) => {
                        if let Err(err) = tx.send(block) {
                            debug!(?err, "failed to send block")
                        }
                    }
                    Err(err) => {
                        debug!(?err, "synchronize error");
                        break;
                    }
                }
            }

            debug!("synchronize thread exit");
        })
        .unwrap();

    return rx.into_iter();
}

#[allow(unused)]
fn sync_on_main<R>(reader: R, opts: SyncOpts) -> impl Iterator<Item = Cadu>
where
    R: Read + Send + 'static,
{
    let sync = Synchronizer::new(reader, opts.length).with_asm(opts.asm);

    sync.into_iter().filter_map(Result::ok)
}

/// Options used for synchronization
pub struct SyncOpts<'a> {
    asm: &'a [u8],
    length: usize,
}

impl<'a> SyncOpts<'a> {
    /// Create a new set of sync options.
    ///
    /// # Arguments
    /// * `length` Length of data to return as a [Block], not including the length of the attached
    /// sync marker.
    pub fn new(length: usize) -> Self {
        SyncOpts { asm: &ASM, length }
    }

    /// Attached sync marker indicating the start of a [Block].
    pub fn with_asm(mut self, asm: &'a [u8]) -> Self {
        self.asm = asm;
        self
    }
}

/// Syncronize a bit stream to provide a byte-aligned iterator of [Block] data.
pub fn synchronize<R>(reader: R, opts: SyncOpts) -> impl Iterator<Item = Cadu>
where
    R: Read + Send + 'static,
{
    // sync_on_thread(reader, block_length, 10)
    sync_on_main(reader, opts)
}
