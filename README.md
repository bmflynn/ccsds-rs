# ccsds

## CCSDS Spacecraft Data Stream Decoding

The project provides tools for decoding spacecraft downlink telemetry streams conforming
to the [`CCSDS`] recommended specifications (Blue Books)
[`TM Synchronization and Channel Coding`] and [`Space Packet Protocol`].

Supports:
- Framing
    - Stream synchronization
    - Pseudo-noise removal
    - Reed-Solomon FEC
- Spacepacket decoding
    - Telemetry packets, i.e., packets with type 0
    - Sequencing
    - Packet groups
- Limited support for secondary header timecodes
    - CCSDS Day Segmented timecodes
    - NASA EOS timecodes for Aqua and Terra spacecrafts
    - Provided but not directly used

### Examples
The following example shows how to decode an unsynchrozied byte stream of CADUs for
the Suomi-NPP spacecraft. This example code should work for any spacecraft data stream
that conforms to CCSDS [`TM Synchronization and Channel Coding`] and [`Space Packet Protocol`]
documents.
```rust
use std::fs;
use ccsds::{FrameDecoderBuilder, decode_framed_packets, collect_packet_groups, PacketGroup};

let file = fs::File::open("snpp.dat")
    .expect("failed to open data file");
let frames = FrameDecoderBuilder::new(1024)
    .reed_solomon_interleave(4)
    .build(file);
// Suomi-NPP has 0 length izone and trailer
let packets = decode_framed_packets(157, Box::new(frames), 0, 0);

// The VIIRS sensor on Suomi-NPP uses packet grouping, so here we collect the packets
// into their associated groups.
let groups: Vec<PacketGroup> = collect_packet_groups(Box::new(packets))
    .filter_map(|zult| zult.ok())
    .collect();
```

### References:
* [`CCSDS`]
* [`Space Packet Protocol`]
* [`TM Synchronization and Channel Coding`]
* [`TM Synchronization and Channel Coding - Summary of Concept and Rationale`]


### License

GNU General Public License v3.0

[`CCSDS`]: https://public.ccsds.org
[`Space Packet Protocol`]: https://public.ccsds.org/Pubs/133x0b1c2.pdf
[`TM Synchronization and Channel Coding`]: https://public.ccsds.org/Pubs/131x0b5.pdf
[`TM Synchronization and Channel Coding - Summary of Concept and Rationale`]: https://public.ccsds.org/Pubs/130x1g3.pdf
[Level-0]: https://www.earthdata.nasa.gov/engage/open-data-services-and-software/data-information-policy/data-levels
[VIIRS]: https://www.star.nesdis.noaa.gov/jpss/VIIRS.php
