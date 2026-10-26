// Layout checks for the PipeWire FFI bindings behind `scap`.
//
// With Clang >= 22, bindgen < 0.72.1 turned these structs into opaque
// one-byte stubs (`_address: u8`, rust-bindgen#3275). For `spa_pod_builder`
// that broke the build, but `pw_mempool` / `pw_memmap` miscompiled with no error. These
// tests fail if the generated bindings go opaque again.
//
// Expected values come from the C headers (gcc `sizeof` / `offsetof`,
// PipeWire 1.6.10, x86_64). Only `spa_pod_builder` is checked exactly: its
// layout has been stable since PipeWire 0.3. The others are only checked
// for "not an opaque stub", because their size may differ between
// PipeWire releases.

use std::mem::{align_of, offset_of, size_of};

use libspa_sys::{
    spa_buffer, spa_handle, spa_meta_header, spa_pod_builder, spa_ringbuffer, spa_source,
};
use pipewire_sys::{pw_buffer, pw_memmap, pw_mempool};

#[test]
#[cfg(target_pointer_width = "64")]
fn spa_pod_builder_matches_c_layout() {
    // struct spa_pod_builder { void *data; uint32_t size; uint32_t _padding;
    //   struct spa_pod_builder_state state; struct spa_callbacks callbacks; };
    assert_eq!(size_of::<spa_pod_builder>(), 48);
    assert_eq!(align_of::<spa_pod_builder>(), 8);
    assert_eq!(offset_of!(spa_pod_builder, data), 0);
    assert_eq!(offset_of!(spa_pod_builder, size), 8);
    assert_eq!(offset_of!(spa_pod_builder, state), 16);
    assert_eq!(offset_of!(spa_pod_builder, callbacks), 32);
}

#[test]
fn previously_opaque_structs_have_real_layout() {
    // Smallest real one is spa_ringbuffer (two uint32_t = 8 bytes); an opaque
    // stub is 1 byte.
    let sizes = [
        ("spa_ringbuffer", size_of::<spa_ringbuffer>()),
        ("spa_handle", size_of::<spa_handle>()),
        ("spa_source", size_of::<spa_source>()),
        ("spa_buffer", size_of::<spa_buffer>()),
        ("spa_meta_header", size_of::<spa_meta_header>()),
        ("pw_mempool", size_of::<pw_mempool>()),
        ("pw_memmap", size_of::<pw_memmap>()),
        ("pw_buffer", size_of::<pw_buffer>()),
    ];
    for (name, size) in sizes {
        assert!(size >= 8, "{name} is {size} bytes: bindings look opaque");
    }
}
