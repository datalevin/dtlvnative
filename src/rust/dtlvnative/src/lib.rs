//! Native interfaces for the Rust Datalevin core.
//!
//! The initial implementation provides DLMDB storage. Vector and model runtime
//! interfaces are implemented in later phases of the repository's Rust plan.

pub use dtlvnative_sys as sys;

#[cfg(feature = "dlmdb")]
pub mod dlmdb;
