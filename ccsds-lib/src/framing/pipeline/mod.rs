mod builder;
mod framing;
mod packets;
mod reedsolomon;
mod synchronize;

pub use builder::*;
pub use framing::*;
pub use packets::*;
pub use reedsolomon::*;
pub use synchronize::*;

use super::{synchronizer::Block, DefaultDerandomizer, Derandomizer};

pub type Cadu = Block;

pub fn derandomize<I>(cadus: I) -> impl Iterator<Item = Cadu>
where
    I: Iterator<Item = Cadu>,
{
    let pn = DefaultDerandomizer::default();

    cadus.map(move |mut cadu| {
        cadu.data = pn.derandomize(&cadu.data);
        cadu
    })
}
