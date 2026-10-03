# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.2.1](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-api-v2.2.0...rusty_alloc-api-v2.2.1) - 2026-09-25

### Performance

- **`GlobalAlloc` methods are `#[inline]`**, as the `mimalloc` crate's are, so
  rustc's `__rust_alloc` / `__rust_dealloc` shims carry the fast path instead
  of jumping to it, and `dealloc` is the free fast path with its null test
  folded away. Measured whole-program against 2.2.0 on two deterministic Rust
  workloads (`bench/rust-globalalloc.sh`): **-21.1 %** and **-5.8 %**.
- **Layouts aligned up to two words (16 bytes on 64-bit) come from the natural
  size classes**, which are already aligned that far, instead of the aligned
  path; every hashbrown table is such a layout. `tests/natural_align.rs` pins
  the alignment for every size class through alloc, alloc_zeroed and realloc.
- **`realloc` keeps a block in place at any alignment when it fits** instead
  of always allocating, copying and freeing above 8 bytes of alignment:
  **-27.3 %** on an over-aligned buffer workload.

## [2.2.0](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-api-v2.1.0...rusty_alloc-api-v2.2.0) - 2026-09-10

### Other

- release v2.2.0

## [2.1.0](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-api-v2.0.5...rusty_alloc-api-v2.1.0) - 2026-09-10

### Other

- release v2.1.0

## [2.0.5](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-api-v2.0.4...rusty_alloc-api-v2.0.5) - 2026-09-09

### Other

- release v2.0.5

## [2.0.0](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-api-v1.1.6...rusty_alloc-api-v2.0.0) - 2026-09-07

### Breaking

- **`default-features = false` now selects `no_std`.** This crate's own source
  has been `no_std` since M2; only the core dependency's default features stood
  in the way. `default = ["std"]` is new, and turning it off gives a firmware
  the single-heap profile — which also requires `--cfg ra_single_threaded` on
  the core. Consumers on default features are unaffected.

### Added

- `std` feature (default-on) forwarding to `rusty_alloc/std`, so
  `RustyAlloc` can be a bare-metal `#[global_allocator]`.

## [1.1.6](https://github.com/Remade-With-Rust/rusty_alloc/compare/rusty_alloc-api-v1.1.5...rusty_alloc-api-v1.1.6) - 2026-08-28

### Other

- release v1.1.6 ([#11](https://github.com/Remade-With-Rust/rusty_alloc/pull/11))
