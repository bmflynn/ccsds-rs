# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Workspace layout

Cargo workspace with three crates. `default-members` is `ccsds-cmd` and `ccsds-lib`, so plain `cargo build` and `cargo test` skip `ccsds-py`.

- `ccsds-lib` (package and lib name `ccsds`): decoding library. Framing, spacepackets, timecodes.
- `ccsds-cmd` (binary `ccsds`): clap CLI built on the library. Subcommands `frames` and `packets`.
- `ccsds-py`: pyo3 Python bindings, built with maturin.

Spacecraft metadata (frame lengths, VCIDs, APID and timecode config) comes from the external `spacecrafts` crate, a git dependency on `bmflynn/spacecrafts-rs` with no pinned rev in `Cargo.toml`. `Cargo.lock` holds the real pin. The CLI loads spacecraft config by path, or by name from `~/.config/ccsds/spacecrafts/`.

## Commands

```sh
cargo build                          # ccsds-cmd + ccsds-lib
cargo test                           # same members; CI runs exactly this
cargo test -p ccsds <name>           # one test in the library
cargo test -p ccsds --test framing   # one integration test file
cargo bench -p ccsds                 # criterion benches (ccsds-lib/benches)
cargo clippy --all-targets
./scripts/make_dist.sh               # static musl release tarball of the CLI
```

Python bindings (not covered by the default workspace commands):

```sh
cd ccsds-py
maturin develop        # build and install into the active venv
pytest tests/test.py
```

## Git LFS

The large fixtures use Git LFS (`.gitattributes`): `ccsds-lib/tests/fixtures/cadu/*.dat`, `viirs_*.dat`, and `ccsds-lib/benches/snpp_block.dat`. Without `git lfs pull`, tests that read them fail on pointer files. CI runs `git lfs pull` before `cargo test`.

## Architecture

### `ccsds-lib::framing`

Built as a chain of iterator adapters. Each stage takes an `Iterator` and returns an `Iterator`:

`synchronize` (find ASM, yield `Block`/`Cadu`) -> `derandomize` (PN removal) -> `frame_decoder` (`Cadu` -> `Frame`) -> `reed_solomon` (adds `Frame.integrity`, optional correction, uses threads) -> `PacketDemux` (frames to `Packet`s per VCID).

The stage functions live in `framing/pipeline/`. The trait-based implementations (`DefaultDerandomizer`, `DefaultReedSolomon`, synchronizer) sit one level up in `framing/`. `pipeline::Pipeline` is a builder that wires the common chain and returns a boxed `Iterator<Item = Frame>`. Its block length is the CADU length minus the ASM length.

`VCDUHeader::decode` accepts header versions 0 and 1 only. The docs say AOS is supported and TM (CCSDS 132.0-B-3) is not, but the comments in `decode_v1` and `decode_v2` and the doc comment on `decode` disagree about which spec each version follows. Check them before changing header parsing. `Frame::mpdu` takes `izone_length`, `trailer_length` and `fhec` as arguments, so callers must supply the per-spacecraft frame layout.

### `ccsds-lib::spacepacket` and `timecode`

`Packet` holds the primary header plus raw bytes. `spacepacket/merge.rs` and `summary.rs` provide time-ordered merge and per-APID summaries. `timecode/` has CDS and CUC decoders. `spacepacket/timecode.rs` builds a per-APID time decoder from spacecraft config. Merge needs an 8 byte CDS timecode at the start of the secondary header.

CUC `fine_nanos` of 0 means 1 ns per fine count. The rule lives in `decode_cuc` (`timecode/cuc.rs`), so a config that omits `fineNanos` still decodes fine time.

### `ccsds-cmd`

`main.rs` defines the clap tree and shared `ConfigArgs` (spacecraft config selection). `frame/` and `packet/` hold one file per subcommand. Packet handling for each VCID uses the per-channel framing config from the spacecraft definition (`packet/demux.rs`).

### `ccsds-py`

Thin wrappers. `ccsds-py/Cargo.toml` takes `spacecrafts` from crates.io (`^0.1.0-beta.3`), not the workspace git dependency used by the other crates. The versions can differ, so check this if the Python build fails on spacecraft config types. `ccsds-lib` types gain `#[pyclass]` only under the `python` cargo feature (`cfg_attr(feature = "python", ...)`), so core types carry pyo3 attributes inline. `ccsds-py/ccsds.pyi` is the hand-written type stub. Update it when the Python API changes.

## Conventions

- Public library API returns iterators. Keep new pipeline stages lazy and composable.
- Library errors use `thiserror` (`ccsds::Error`, `ccsds::Result`). The CLI uses `anyhow`.
- The README warns that the API is unstable. Breaking changes are acceptable.
