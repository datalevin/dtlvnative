//! Raw bindings generated from the DLMDB fork and DTLV storage headers.
//!
//! Every native call requires the caller to uphold the native ownership,
//! transaction, threading, and buffer validity contracts. No stock LMDB library
//! may be substituted for the bundled DLMDB source.

#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

/// Source revision and configuration observed when building this crate.
pub const BUILD_INFO: &str = include_str!(concat!(env!("OUT_DIR"), "/build-info.txt"));

#[cfg(feature = "dlmdb")]
mod bindings {
    #![allow(clippy::all, clippy::undocumented_unsafe_blocks)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[cfg(feature = "dlmdb")]
pub use bindings::*;
