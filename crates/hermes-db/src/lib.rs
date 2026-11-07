//! Hermes encrypted event store.
//!
//! * [`Database`]: SQLite open/migrate/recover, key registry, sessions.
//! * [`EventStore`]: collector-side bounded, batched, encrypt-on-push writer (public key only).
//! * [`ArchiveReader`] + [`Keyring`]: viewer-side read access and decryption.
#![forbid(unsafe_code)]

mod db;
mod error;
mod reader;
pub mod schema;
mod store;

pub use db::{now_us, Database, KeyRecord, OpenOptions, OpenReport, RecoveryReport};
pub use error::{Result, StoreError};
pub use reader::{
    ArchiveReader, DecryptedEvent, EventQuery, IntegrityReport, Keyring, OpenEventError,
    StoredCheckpoint, StoredEvent,
};
pub use schema::kind;
pub use store::{new_session_id, EventStore, PushOutcome, StoreConfig};

#[cfg(test)]
mod tests;
