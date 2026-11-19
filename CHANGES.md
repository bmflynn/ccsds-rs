# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED]

### Added

- `Packet::vcid` and `PacketGroup::vcid` (`Option<Vcid>`). `PacketDemux` sets the VCID. Packets
  decoded from bytes have `None`. This supports the same APID on different VCIDs.
- `PacketKey` and `PacketApidTimeDecoder::add_for_vcid`.
- `Derandomizer::derandomize_inplace` removes pseudo-noise without allocating a new buffer. The
  `derandomize` pipeline stage and `ccsds frames sync` use it. This is a breaking change for
  custom `Derandomizer` implementations, which must add the method.
- `ccsds frames sync` logs each found block at debug level.
- `OpaqueFrame` and `opaque_frame_decoder` for frames of unknown format. `reed_solomon` accepts
  either `Frame` or `OpaqueFrame` via the new `RsFrame` trait.
- `VCDUHeader::is_aos`, `is_tm` and `is_opaque`.
- `ccsds spacecrafts list` and `ccsds spacecrafts show` print spacecraft configuration as text
  tables.

### Changed

- `Summary::apids` is keyed by `PacketKey` (`vcid`, `apid`) and serializes as a list of pairs.
- `PacketApidTimeDecoder::with_spacecraft` uses the spacecraft config VCID. Packets with no VCID
  use the first decoder configured for the APID.
- Packet groups and `Merger` de-duplication consider the VCID as well as the APID.
- Increase synchronize troughput significantly
- `Frame::decode` takes `&[u8]` instead of `Vec<u8>`.
- `VCDUHeader::decode` no longer returns `None` for an unknown version; it decodes as version 1
  (AOS) and keeps the version value.
- `reed_solomon` no longer skips fill frames.
- Packet decoding no longer drops packets with a non-zero version or type flag. The check was too
  strict. Callers that need it must validate the primary header themselves.

### Removed

- Python bindings (`ccsds-py`) and the `python` feature on `ccsds`.
- Unused `ccsds` dependencies.

### Fixed

- Bad default CUC fine_nanos, and associated incorrect param name
- `ccsds frames sync` no longer prints an error when the input ends normally (unexpected EOF).
- Doc comments for `Loc::offset` (1-based) and `Loc::bit` now describe the actual behavior.
