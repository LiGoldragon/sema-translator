//! Sole-writer runtime for the SEMA naming translator.
//!
//! One actor owns one embedded SEMA database. The runtime persists only the
//! complete nested [`signal_sema_translator::VocabularyNameTable`] archive,
//! trusted Rust-vocabulary release metadata, and authority-level idempotency
//! receipts. The bootstrap assembly boundary authenticates caller-supplied
//! opaque identity seats transiently; component documents never enter the
//! authority store.

mod authorization;
pub mod bootstrap;
mod runtime;
mod store;
pub mod wire;

pub use authorization::{AuthorizationPolicy, StaticAuthorizationPolicy, principal_for_unix_uid};
pub use runtime::{DispatchOutcome, Runtime};
pub use store::{Error, Result};

use signal_frame::{RootCode, VariantCode, WireRoute};

/// Stable contract-local route used by the authority process.
///
/// The contract binding identifies the archived body. This route only
/// identifies the authority request family within that already-bound contract.
pub const AUTHORITY_ROUTE: WireRoute = WireRoute::new(RootCode::new(1), VariantCode::new(1));

/// Approved runtime directory name.
pub const RUNTIME_DIRECTORY_NAME: &str = "sema-translator";

/// Approved daemon binary name.
pub const DAEMON_BINARY_NAME: &str = "sema-translator-daemon";

/// Approved deployment service name.
pub const SERVICE_NAME: &str = "sema-translator-daemon.service";

/// Approved local socket basename.
pub const SOCKET_FILE_NAME: &str = "sema-translator.sock";

/// Approved embedded database basename.
pub const DATABASE_FILE_NAME: &str = "sema-translator.sema";
