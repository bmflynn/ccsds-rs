#![doc = include_str!("../README.md")]

mod error;

pub mod config;
pub mod framing;
pub mod spacepacket;
pub mod timecode;

pub use error::{Error, Result};
