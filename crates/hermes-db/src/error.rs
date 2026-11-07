use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] hermes_crypto::CryptoError),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database schema version {found} is newer than this build supports ({supported}); refusing to open")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("file is a SQLite database but not a Hermes database (application_id {0:#x})")]
    NotHermesDatabase(i64),
    #[error(
        "no active encryption key is registered; refusing to store records without encryption"
    )]
    NoActiveKey,
    #[error("key {0} is already registered")]
    KeyExists(String),
    #[error("the active key set has no registered signing key (created before signed checkpoints); run `hermes-keytool rotate` to create a signed key set")]
    KeySetNotSigned,
    #[error(
        "the loaded signing key does not match the signing key registered for the active key set"
    )]
    SignerMismatch,
    #[error("event is too large")]
    EventTooLarge,
    #[error("migration {version} failed: {source}")]
    Migration {
        version: u32,
        source: rusqlite::Error,
    },
    #[error("database is corrupt: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// True when a sqlite error means "the file's content is damaged" (as opposed to busy/locked/io).
pub(crate) fn is_corruption(e: &rusqlite::Error) -> bool {
    use rusqlite::ErrorCode::*;
    matches!(
        e.sqlite_error_code(),
        Some(DatabaseCorrupt) | Some(NotADatabase)
    )
}
