//! # CCSDS Spacecraft Data Stream Decoding
//!
//! The project provides tools for decoding spacecraft downlink telemetry streams conforming
//! to the [`CCSDS`] recommended specifications (Blue Books)
//! [`TM Synchronization and Channel Coding`] and [`Space Packet Protocol`].
//!
//! Supports:
//! - Framing
//!     - Stream synchronization
//!     - Pseudo-noise removal
//!     - Reed-Solomon FEC
//! - Spacepacket decoding
//!     - Telemetry packets, i.e., packets with type 0
//!     - Sequencing
//!     - Packet groups
//! - Limited support for secondary header timecodes
//!     - CCSDS Day Segmented timecodes
//!     - NASA EOS timecodes for Aqua and Terra spacecrafts
//!     - Provided but not directly used
//!
//! ## Examples
//! The following example shows how to decode an unsynchrozied byte stream of CADUs for
//! the Suomi-NPP spacecraft. This example code should work for any spacecraft data stream
//! that conforms to CCSDS [`TM Synchronization and Channel Coding`] and [`Space Packet Protocol`]
//! documents.
//! ```no_run
//! use std::fs::File;
//! use std::io::BufReader;
//! use ccsds::{ASM, FrameDecoderBuilder, Synchronizer, decode_framed_packets, collect_packet_groups, PacketGroup};
//!
//! // 1. Synchronize stream and extract blocks (CADUs w/o ASM)
//! let file = BufReader::new(File::open("snpp.dat")
//!     .expect("failed to open data file"));
//! let blocks = Synchronizer::new(file, &ASM.to_vec(), 1020)
//!     .into_iter()
//!     .filter_map(Result::ok);
//!
//! // 2. Decode those blocks into Frames
//! let frames = FrameDecoderBuilder::default()
//!     .reed_solomon_interleave(4)
//!     .build(blocks);
//!
//! // 3. Extract packets from Frames
//! // Suomi-NPP has 0 length izone and trailer
//! let packets = decode_framed_packets(157, Box::new(frames), 0, 0);
//! ```
//!
//! ## References:
//! * [`CCSDS`]
//! * [`Space Packet Protocol`]
//! * [`TM Synchronization and Channel Coding`]
//! * [`TM Synchronization and Channel Coding - Summary of Concept and Rationale`]
//!
//!
//! ## License
//!
//! GNU General Public License v3.0
//!
//! [`CCSDS`]: https://public.ccsds.org
//! [`Space Packet Protocol`]: https://public.ccsds.org/Pubs/133x0b1c2.pdf
//! [`TM Synchronization and Channel Coding`]: https://public.ccsds.org/Pubs/131x0b5.pdf
//! [`TM Synchronization and Channel Coding - Summary of Concept and Rationale`]: https://public.ccsds.org/Pubs/130x1g3.pdf
//! [Level-0]: https://www.earthdata.nasa.gov/engage/open-data-services-and-software/data-information-policy/data-levels
//! [VIIRS]: https://www.star.nesdis.noaa.gov/jpss/VIIRS.php

mod bytes;
mod framing;
mod pn;
mod rs;
mod spacepacket;
mod synchronizer;
pub mod timecode;

pub use framing::*;
pub use rs::{
    correct_message as rs_correct_message, deinterleave as rs_deinterleave,
    has_errors as rs_has_errors, DefaultReedSolomon, RSState, ReedSolomon,
};
pub use spacepacket::*;
pub use synchronizer::{read_synchronized_blocks, Synchronizer, ASM};
