# pipewire-sys 0.8.0 (vendored)

- **Origin:** upstream pipewire-rs, <https://gitlab.freedesktop.org/pipewire/pipewire-rs>,
  directory `pipewire-sys/` at commit `93138d01b23628521b29b5604bbebe991cba4c65`
  ("Update dependencies and migrate to nom 8 API", 2025-08-06). At that commit
  the workspace version is still 0.8.0; the next release (0.9.0) shipped it.
- **Licence:** MIT, unchanged. `LICENSE` is upstream's repository-level
  licence file (in the repo it is a symlink to `../LICENSE`; here it is a copy).
  Identical to the `LICENSE` in the crates.io `pipewire-sys` 0.8.0 package.
- **Reason:** bindgen 0.72 bump. The crates.io 0.8.0 release builds with
  bindgen 0.69. Since Clang 22 removed `ElaboratedType`
  (llvm/llvm-project#147835), bindgen < 0.72.1 emits opaque one-byte stubs for
  structs reached through a typedef (rust-bindgen#3275, fixed by #3278 in
  0.72.1): `spa_pod_builder`, `spa_ringbuffer`, `spa_handle`, `spa_source`,
  `pw_mempool`, `pw_memmap`, ... libspa 0.8 then fails to compile ("no field
  `data` on type `spa_pod_builder`") and pipewire-sys silently gets wrong
  layouts. Upstream's manifest asks for bindgen 0.72.0; Cargo.lock pins
  0.72.1 and must stay >= 0.72.1. Kaiku cannot move to pipewire >= 0.9 yet
  (scap pins 0.8; libspa >= 0.9 conflicts with fred's cookie-factory pin), so
  only the -sys crates are replaced via `[patch.crates-io]` in the workspace
  `Cargo.toml`. Guarded by `client/src-tauri/src/capture/pipewire_ffi_tests.rs`.

## Changes against upstream at that commit

- `Cargo.toml`: workspace-inherited keys (`version`, `authors`, `edition`,
  `rust-version`, `license`, `repository`, `homepage`, `categories`) written
  out with the upstream workspace values; `readme` points to
  `README.upstream.md`; `publish = false`; comment header.
- `README.md` renamed to `README.upstream.md`; this file added.
- `LICENSE` symlink replaced by the file it points to.

All other files are byte-identical to upstream. Compared to the crates.io 0.8.0
release, the code changes are bindgen 0.69 -> 0.72, system-deps 6 -> 7, a
dropped `&` in build.rs and CStr literals in tests.

## Removal

Drop this directory and its line in the workspace `[patch.crates-io]` once
scap (and therefore kaiku) uses pipewire >= 0.9.
