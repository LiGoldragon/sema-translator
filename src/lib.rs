//! Authority-approved bootstrap translation for strict Ethos assembly.
//!
//! The crate accepts explicit, already-minted identity seats and binds them to
//! one exact prepared bootstrap transaction. It does not own a runtime engine,
//! database, actor, daemon, or wire protocol.

#[cfg(feature = "bootstrap")]
pub mod bootstrap;

#[cfg(all(test, feature = "bootstrap"))]
#[path = "../tests/bootstrap.rs"]
mod bootstrap_tests;
