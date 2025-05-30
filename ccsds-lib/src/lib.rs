#![doc = include_str!("../README.md")]

mod error;

pub mod framing;
pub mod spacecrafts;
pub mod spacepacket;
pub mod timecode;

pub use error::{Error, Result};
