//! Authority-approved bootstrap translation for strict Ethos assembly.
//!
//! Sema-owned bootstrap identity authority for strict Ethos assembly.
//!
//! Callers supply source text and source placement only. This crate privately
//! mints `EncodedName` values, stages canonical textual metadata and receipts,
//! and invokes direct strict-value `TrueNamed` identities. It does not own a
//! runtime engine, database, actor, daemon, or wire protocol.

#[cfg(feature = "bootstrap")]
pub mod bootstrap;

#[cfg(all(test, feature = "bootstrap"))]
#[path = "../tests/bootstrap.rs"]
mod bootstrap_tests;
