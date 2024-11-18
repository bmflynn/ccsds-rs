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
//!     - Telemetry packets
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
//! documents, where the input data is a stream containing pseudo-randomized CADUs with
//! Reed-Solomon FEC (including parity bytes).
//!
//! ```no_run
//! use std::fs::File;
//! use std::io::BufReader;
//! use ccsds::framing::*;
//!
//! // Framing configuration
//! let block_len = 1020; // CADU length - ASM length
//! let interleave: u8 = 4;
//! let izone_len = 0;
//! let trailer_len = 0;
//!
//! // 1. Synchronize stream and extract blocks
//! let file = BufReader::new(File::open("snpp.dat").unwrap());
//! let cadus = Synchronizer::new(file, &ASM.to_vec(), block_len)
//!     .into_iter()
//!     .filter_map(Result::ok);
//!
//! // 2. Decode (PN & RS) those blocks into Frames, ignoring frames with errors
//! let frames = decode_frames(
//!     cadus,
//!     Some(Box::new(DefaultReedSolomon::new(interleave))),
//!     Some(Box::new(DefaultDerandomizer)),
//! ).filter_map(Result::ok);
//!
//! // 3. Extract packets from Frames
//! let packets = decode_framed_packets(frames, izone_len, trailer_len);
//! ```
//!
//! It is also possible to have more control over the decode process for cases that do not
//! conform to the standard CCSDS specifications.
//!
//! For example, this will decode a stream of frames that are not pseudo-randomized and does
//! not use Reed-Solomon FEC.
//! ```no_run
//! use std::fs::File;
//! use std::io::BufReader;
//! use ccsds::framing::*;
//!
//! let block_len = 892; // Frame length
//! let interleave: u8 = 4;
//! let izone_len = 0;
//! let trailer_len = 0;
//!
//! // 1. Synchronize stream and extract blocks
//! let file = BufReader::new(File::open("frames.dat").unwrap());
//! let cadus = Synchronizer::new(file, &ASM.to_vec(), block_len)
//!     .into_iter()
//!     .filter_map(Result::ok);
//!
//! // 2. Decode blocks into Frames
//! let frames = decode_frames(cadus, None, None).filter_map(|z| z.ok());
//!
//! // 3. Extract packets from Frames
//! let packets = decode_framed_packets(frames, izone_len, trailer_len);
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

mod error;

pub mod framing;
pub mod prelude;
pub mod spacecrafts;
pub mod spacepacket;
pub mod timecode;

pub use error::{Error, Result};
