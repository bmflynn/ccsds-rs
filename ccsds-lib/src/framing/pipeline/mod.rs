mod builder;
mod framing;
mod packets;
mod reed_solomon;
mod synchronize;

pub use builder::*;
pub use framing::*;
pub use packets::*;
pub use reed_solomon::*;
pub use synchronize::*;

use super::{Cadu, DefaultDerandomizer, Derandomizer};

/// Perform derandomization on each input [Cadu] using [DefaultDerandomizer].
pub fn derandomize<I>(cadus: I) -> impl Iterator<Item = Cadu>
where
    I: Iterator<Item = Cadu>,
{
    let pn = DefaultDerandomizer;

    cadus.map(move |mut cadu| {
        pn.derandomize_inplace(&mut cadu.data);
        cadu
    })
}
