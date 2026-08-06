//! Sole-writer runtime for the SEMA naming translator.
//!
//! One actor owns one embedded SEMA database. The runtime persists only the
//! complete nested [`signal_sema_translator::VocabularyNameTable`] archive,
//! trusted Rust-vocabulary release metadata, and authority-level idempotency
//! receipts. The bootstrap assembly boundary authenticates caller-supplied
//! opaque identity seats transiently; component documents never enter the
//! authority store.
//!
//! The `bootstrap` feature exposes that production authority boundary without
//! the SEMA runtime engine. The `runtime` feature owns the actor, store, wire,
//! and daemon surfaces. Default builds enable both and preserve the complete
//! product.

#[cfg(feature = "runtime")]
mod authorization;
#[cfg(feature = "bootstrap")]
pub mod bootstrap;
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
mod store;
#[cfg(feature = "runtime")]
pub mod wire;

#[cfg(feature = "runtime")]
pub use authorization::{AuthorizationPolicy, StaticAuthorizationPolicy, principal_for_unix_uid};
#[cfg(feature = "runtime")]
pub use runtime::{DispatchOutcome, Runtime};
#[cfg(feature = "runtime")]
pub use store::{Error, Result};

#[cfg(feature = "runtime")]
use signal_frame::{RootCode, VariantCode, WireRoute};

/// Stable contract-local route used by the authority process.
///
/// The contract binding identifies the archived body. This route only
/// identifies the authority request family within that already-bound contract.
#[cfg(feature = "runtime")]
pub const AUTHORITY_ROUTE: WireRoute = WireRoute::new(RootCode::new(1), VariantCode::new(1));

/// Approved runtime directory name.
#[cfg(feature = "runtime")]
pub const RUNTIME_DIRECTORY_NAME: &str = "sema-translator";

/// Approved daemon binary name.
#[cfg(feature = "runtime")]
pub const DAEMON_BINARY_NAME: &str = "sema-translator-daemon";

/// Approved deployment service name.
#[cfg(feature = "runtime")]
pub const SERVICE_NAME: &str = "sema-translator-daemon.service";

/// Approved local socket basename.
#[cfg(feature = "runtime")]
pub const SOCKET_FILE_NAME: &str = "sema-translator.sock";

/// Approved embedded database basename.
#[cfg(feature = "runtime")]
pub const DATABASE_FILE_NAME: &str = "sema-translator.sema";
