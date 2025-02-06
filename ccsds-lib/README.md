# CCSDS Spacecraft Data Stream Decoding

> **WARNING**: 
> This project is very much in development, and the API is very likely to change in ways that will 
> break things. If you have comments or suggestions regarding the API feel free to file an issue.

The project provides tools for decoding spacecraft downlink telemetry streams conforming
to the [`CCSDS`] recommended specifications (Blue Books)
[`TM Synchronization and Channel Coding`] and [`Space Packet Protocol`].

Supports:
- Framing
    - Stream synchronization
    - Deransomization (pseudo-noise removal)
    - Integrity checking/correcting
        * Reed-Solomon FEC
        * CRC
- Spacepacket decoding
    - Telemetry packets
    - Sequencing
    - Packet groups
- Limited support for secondary header timecodes
    - CCSDS Day Segmented timecodes
    - NASA EOS timecodes for Aqua and Terra spacecrafts
    - Provided but not directly used

Much of the functionality is wrapped around [Iterator]s, and as such most of the public API 
returns an [Iterator] of some sort. 

## Examples
The following example shows how to decode an unsynchronized byte stream of CADUs for
the Suomi-NPP spacecraft. This example code should work for any spacecraft data stream
that conforms to CCSDS [`TM Synchronization and Channel Coding`] and [`Space Packet Protocol`]
documents, where the input data is a stream containing pseudo-randomized CADUs with
Reed-Solomon FEC (including parity bytes).

```no_run
use std::fs::File;
use std::io::BufReader;
use ccsds::framing::*;

// Framing configuration
let block_len = 1020; // CADU length - ASM length
let interleave: u8 = 4;
let izone_len = 0;
let trailer_len = 0;

// 1. Synchronize stream and extract blocks
let file = BufReader::new(File::open("snpp.dat").unwrap());
let cadus = Synchronizer::new(file, block_len)
     .into_iter()
     .filter_map(Result::ok);

// 2. Decode (PN & RS) those blocks into Frames, ignoring frames with errors
let frames = decode_frames(
    cadus,
    Some(Box::new(DefaultReedSolomon::new(interleave))),
    Some(Box::new(DefaultDerandomizer)),
).filter_map(Result::ok);

// 3. Extract packets from Frames
let packets = decode_framed_packets(frames, izone_len, trailer_len);
```

It is also possible to have more control over the decode process for cases that do not
conform to the standard CCSDS specifications.

For example, this will decode a stream of frames that are not pseudo-randomized and does
not use Reed-Solomon FEC.
```no_run
use std::fs::File;
use std::io::BufReader;
use ccsds::framing::*;

let block_len = 892; // Frame length
let interleave: u8 = 4;
let izone_len = 0;
let trailer_len = 0;

// 1. Synchronize stream and extract blocks
let file = BufReader::new(File::open("frames.dat").unwrap());
let cadus = Synchronizer::new(file, block_len)
    .into_iter()
    .filter_map(Result::ok);

// 2. Decode blocks into Frames
let frames = decode_frames(cadus, None, None).filter_map(|z| z.ok());

// 3. Extract packets from Frames
let packets = decode_framed_packets(frames, izone_len, trailer_len);
```

## References:
* [`CCSDS`]
* [`Space Packet Protocol`]
* [`TM Synchronization and Channel Coding`]
* [`TM Synchronization and Channel Coding - Summary of Concept and Rationale`]

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

[`CCSDS`]: https://public.ccsds.org
[`Space Packet Protocol`]: https://public.ccsds.org/Pubs/133x0b1c2.pdf
[`TM Synchronization and Channel Coding`]: https://public.ccsds.org/Pubs/131x0b5.pdf
[`TM Synchronization and Channel Coding - Summary of Concept and Rationale`]: https://public.ccsds.org/Pubs/130x1g3.pdf
[Level-0]: https://www.earthdata.nasa.gov/engage/open-data-services-and-software/data-information-policy/data-levels
[VIIRS]: https://www.star.nesdis.noaa.gov/jpss/VIIRS.php
