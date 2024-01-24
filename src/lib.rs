//! CCSDS packet decoding library.
//!
//! Supports:
//! - Framing
//!     - Stream synchronization
//!     - Pseudo-noise removal
//!     - Reed-Solomon RS(223/255) FEC
//! - Spacepacket decoding
//!     - Telemetry packets, i.e., packets with type 0
//!     - Sequencing
//!     - Packet groups
//!     - Some secondary header timecode support
//!
//! The primary way to use this library is through the iterators returned by:
//! - [read_packets]
//! - [read_packet_groups]
//! - [read_synchronized_blocks]
//! - [DecodedFrameIter]
//!
//! References:
//! * [`Space Packet Protocol`](https://public.ccsds.org/Pubs/133x0b1c2.pdf)
//! * [`TM Synchronization and Channel Coding`](https://public.ccsds.org/Pubs/131x0b5.pdf)
//! * [`TM Synchronization and Channel Coding - Summary of Concept and Rationale`](https://public.ccsds.org/Pubs/130x1g3.pdf)

mod bytes;
mod framing;
mod pn;
mod rs;
mod spacepacket;
mod synchronizer;
mod timecode;

pub use framing::*;
pub use rs::{
    correct_message as rs_correct_message, deinterleave as rs_deinterleave,
    has_errors as rs_has_errors, DefaultReedSolomon, RSState, ReedSolomon,
};

pub use spacepacket::*;
pub use synchronizer::{read_synchronized_blocks, Synchronizer, ASM};
