//! Raw bindings generated from the pinned DLMDB, USearch, and DTLV llama headers.
//!
//! Every native call requires the caller to uphold the native ownership,
//! transaction, threading, and buffer validity contracts. No stock LMDB library
//! may be substituted for the versioned DLMDB native artifact.

#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

/// Native artifact source revision, configuration, version, and checksum.
pub const BUILD_INFO: &str = include_str!(concat!(env!("OUT_DIR"), "/build-info.txt"));

#[cfg(feature = "dlmdb")]
mod bindings {
    #![allow(clippy::all, clippy::undocumented_unsafe_blocks)]
    include!(concat!(env!("OUT_DIR"), "/bindings-dlmdb.rs"));
}

#[cfg(feature = "dlmdb")]
pub use bindings::*;

#[cfg(any(feature = "usearch", feature = "llama"))]
pub mod runtime;

#[cfg(feature = "usearch")]
pub mod usearch {
    use std::sync::OnceLock;
    mod bindings {
        #![allow(clippy::all, clippy::undocumented_unsafe_blocks)]
        include!(concat!(env!("OUT_DIR"), "/bindings-usearch.rs"));
    }
    pub use bindings::*;
    static API: OnceLock<Result<UsearchApi, String>> = OnceLock::new();

    /// Load every required symbol from the packaged runtime, once per process.
    pub fn api() -> Result<&'static UsearchApi, &'static str> {
        API.get_or_init(|| {
            let library = super::runtime::load(env!("DTLVNATIVE_USEARCH_LIBRARY"))?;
            // SAFETY: Bindings and runtime come from the same pinned headers and
            // archive version. The generated API owns the loaded library.
            unsafe { UsearchApi::from_library(library).map_err(|error| error.to_string()) }
        })
        .as_ref()
        .map_err(String::as_str)
    }
}

#[cfg(feature = "llama")]
pub mod llama {
    use std::sync::OnceLock;
    mod bindings {
        #![allow(clippy::all, clippy::undocumented_unsafe_blocks)]
        include!(concat!(env!("OUT_DIR"), "/bindings-llama.rs"));
    }
    pub use bindings::*;
    static API: OnceLock<Result<LlamaApi, String>> = OnceLock::new();

    /// Load every required symbol from the packaged runtime, once per process.
    pub fn api() -> Result<&'static LlamaApi, &'static str> {
        API.get_or_init(|| {
            let library = super::runtime::load(env!("DTLVNATIVE_LLAMA_LIBRARY"))?;
            // SAFETY: Bindings and runtime come from the same pinned headers and
            // archive version. The generated API owns the loaded library.
            unsafe { LlamaApi::from_library(library).map_err(|error| error.to_string()) }
        })
        .as_ref()
        .map_err(String::as_str)
    }
}
