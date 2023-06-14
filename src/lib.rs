#![feature(test)]
extern crate test;

pub mod cadu;
/// ccsds packet decoding library.
///
/// References:
/// * CCSDS Space Packet Protocol 133.0-B-1
///     - https://public.ccsds.org/Pubs/133x0b1c2.pdf
///
pub mod error;
pub mod pn;
pub mod rs;
pub mod spacepacket;
