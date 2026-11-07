# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED]

### Added

- Optional `libcorrect` feature on `ccsds` adds `LibcorrectReedSolomon`, a Reed-Solomon backend
  using vendored C code from quiet/libcorrect (new `libcorrect-sys` crate). Select it in the
  pipeline with `RsOpts::with_backend(RsBackend::Libcorrect)`. `rs2` remains the default.

### Changed

- Increase synchronize troughput significantly

### Fixed

- Bad default CUC fine_nanos, and associated incorrect param name
