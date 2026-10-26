# env-libvpx-sys 5.1.3 (vendored)

Source: <https://crates.io/crates/env-libvpx-sys/5.1.3>
(<https://github.com/astraw/env-libvpx-sys>, archived upstream).
Licence: MPL-2.0 (<https://mozilla.org/MPL/2.0/>), unchanged. The files here
remain under MPL-2.0; the rest of the Kaiku repository is not affected.

## Changes against the crates.io release

- `Cargo.toml`: build-dependency `bindgen` 0.68.1 -> 0.72, `publish = false`,
  comment header.
- `generated/` (pre-generated bindings for libvpx 1.3–1.13) is not vendored.
  Kaiku always builds with the `generate` feature (enabled by
  `client/src-tauri/vp8-decoder`), so the build script never reads it; without
  `generate` the build script panics with "Expected file ... not found".
- `README.upstream.md` is the original README.

`build.rs`, `ffi.h`, `src/lib.rs` and `CHANGELOG.md` are byte-identical to the
release.

## Why

bindgen < 0.72 mis-parses the libvpx headers with current libclang (seen with
clang 23 on Arch) and emits `vpx_codec_enc_cfg` / `vpx_codec_dec_cfg` as
opaque one-byte stubs, so `vpx-encode` 0.6 fails to compile
(`no field g_w on type vpx_codec_enc_cfg`).

## Removal

Drop this directory and the `env-libvpx-sys` line in the workspace
`[patch.crates-io]` once a crates.io release with bindgen >= 0.72 exists (or
once vpx-encode / kaiku-vp8-decoder move to a maintained libvpx binding).
