use std::io::Read;

use crate::framing::synchronizer::Synchronizer;

use super::Cadu;

pub fn synchronize<R>(reader: R, block_length: usize) -> impl Iterator<Item = Cadu>
where
    R: Read + Send + 'static,
{
    let sync = Synchronizer::new(reader, block_length);

    sync.into_iter().filter_map(Result::ok)
}
