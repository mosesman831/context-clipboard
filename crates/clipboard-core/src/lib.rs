//! clipboard-core — shared library for Context Clipboard.
//!
//! Provides the pieces both the daemon and the Tauri UI depend on:
//! - [`error`]: the shared [`Error`]/[`Result`] types.
//! - [`paths`]: platform data/runtime/socket/lock paths.
//! - [`config`]: `config.toml` schema and load/save (SPEC §10).
//! - [`crypto`]: AES-256-GCM encrypt/decrypt for clip fields at rest.
//! - [`schema`]: SQLite schema, migrations, and row helpers (SPEC §4).
//! - [`categorize`]: deterministic clip categorization (SPEC §7).
//! - [`ipc`]: versioned length-prefixed JSON protocol (SPEC §9).

pub mod categorize;
pub mod config;
pub mod crypto;
pub mod error;
pub mod ipc;
pub mod paths;
pub mod schema;

pub use categorize::{categorize, Category};
pub use config::Config;
pub use crypto::Key;
pub use error::{Error, Result};
pub use ipc::{
    read_frame, write_frame, ClipDetail, ClipSummary, ExcludedAppInfo, Request, Response,
    IPC_VERSION,
};
pub use schema::{ClipRow, ExcludedApp, SCHEMA_VERSION};
