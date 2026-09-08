//! Native interfaces for the Rust Datalevin core.
//!
//! DLMDB storage is enabled by default. Enable `usearch` for vector indexes,
//! `llama` for CPU model operations, or `full` for all three interfaces.

pub use dtlvnative_sys as sys;

#[cfg(feature = "dlmdb")]
pub mod dlmdb;

#[cfg(feature = "llama")]
pub mod llama;
#[cfg(feature = "usearch")]
pub mod usearch;
