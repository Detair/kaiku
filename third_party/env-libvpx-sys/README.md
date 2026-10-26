# env-libvpx-sys 5.1.3 (vendored)

- **Origin:** <https://crates.io/crates/env-libvpx-sys/5.1.3> (upstream
  <https://github.com/astraw/env-libvpx-sys>, archived; release commit `25fefef`).
- **Reason:** bindgen 0.72 bump (see "Why" below).
- **Licence:** MPL-2.0 (<https://mozilla.org/MPL/2.0/>), unchanged. The files here
  remain under MPL-2.0; the rest of the Kaiku repository is not affected.

## Changes against the crates.io release

- `LICENSE` added: the MPL-2.0 text (SPDX copy). The crate and its upstream
  repository ship no licence file; the licence is declared in `Cargo.toml`.
- `Cargo.toml`: build-dependency `bindgen` 0.68.1 -> 0.72.1, `publish = false`,
  comment header.
- `generated/` (pre-generated bindings for libvpx 1.3–1.13) is not vendored.
  Kaiku always builds with the `generate` feature (enabled by
  `client/src-tauri/vp8-decoder`), so the build script never reads it; without
  `generate` the build script panics with "Expected file ... not found".
- `README.upstream.md` is the original README.

`build.rs`, `ffi.h`, `src/lib.rs` and `CHANGELOG.md` are byte-identical to the
release.

## Why

Clang 22 removed `ElaboratedType` from its AST (llvm/llvm-project#147835).
bindgen before 0.72.1 then treats a struct reached through a typedef
(`typedef struct vpx_codec_enc_cfg {...} vpx_codec_enc_cfg_t;`) as incomplete
and emits an opaque one-byte stub (rust-bindgen#3275, fixed by #3278 in
0.72.1). vpx-encode 0.6 then fails to compile (`no field g_w on type
vpx_codec_enc_cfg`); kaiku-vp8-decoder compiled but with a wrong
`vpx_codec_dec_cfg`. Seen with clang 22+ (Ubuntu 26.04) and 23 (Arch). Older
Clang (e.g. 18 on Ubuntu 24.04) is not affected.

## Removal

Drop this directory and the `env-libvpx-sys` line in the workspace
`[patch.crates-io]` once a crates.io release with bindgen >= 0.72 exists (or
once vpx-encode / kaiku-vp8-decoder move to a maintained libvpx binding).
