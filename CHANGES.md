# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED]

### Added

- Optional `libcorrect` feature on `ccsds` adds `LibcorrectReedSolomon`, a Reed-Solomon backend
  using vendored C code from quiet/libcorrect (new `libcorrect-sys` crate). Select it in the
  pipeline with `RsOpts::with_backend(RsBackend::Libcorrect)`. `rs2` remains the default.

- `Packet::vcid` and `PacketGroup::vcid` (`Option<Vcid>`). `PacketDemux` sets the VCID. Packets
  decoded from bytes have `None`. This supports the same APID on different VCIDs.
- `PacketKey` and `PacketApidTimeDecoder::add_for_vcid`.
- `Derandomizer::derandomize_inplace` removes pseudo-noise without allocating a new buffer. The
  `derandomize` pipeline stage and `ccsds frames sync` use it. This is a breaking change for
  custom `Derandomizer` implementations, which must add the method.
- `ccsds frames sync` logs each found block at debug level.

### Changed

- `Summary::apids` is keyed by `PacketKey` (`vcid`, `apid`) and serializes as a list of pairs.
- `PacketApidTimeDecoder::with_spacecraft` uses the spacecraft config VCID. Packets with no VCID
  use the first decoder configured for the APID.
- Packet groups and `Merger` de-duplication consider the VCID as well as the APID.
- Increase synchronize troughput significantly
- Packet decoding no longer drops packets with a non-zero version or type flag. The check was too
  strict. Callers that need it must validate the primary header themselves.

### Fixed

- Bad default CUC fine_nanos, and associated incorrect param name
- `ccsds frames sync` no longer prints an error when the input ends normally (unexpected EOF).
- Doc comments for `Loc::offset` (1-based) and `Loc::bit` now describe the actual behavior.
