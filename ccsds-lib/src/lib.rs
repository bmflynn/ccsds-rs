#![doc = include_str!("../README.md")]

mod error;

pub mod framing;
pub mod spacepacket;

#[cfg(feature = "timecode")]
pub mod timecode;

pub use error::{Error, Result};
