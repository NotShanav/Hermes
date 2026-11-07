use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("authentication failed: data was modified, bound to a different context, or the key is wrong")]
    AuthenticationFailed,
    #[error(
        "the supplied shares do not reconstruct the expected key (wrong, mixed or corrupted share)"
    )]
    WrongShares,
    #[error("both shares are identical; two distinct shares are required")]
    DuplicateShares,
    #[error("share belongs to key {found} but key {expected} was requested")]
    KeyIdMismatch { expected: String, found: String },
    #[error("malformed share data: {0}")]
    MalformedShare(&'static str),
    #[error("malformed share file: {0}")]
    MalformedFile(&'static str),
    #[error("share file checksum mismatch (file is corrupted or was edited)")]
    ChecksumMismatch,
    #[error("unsupported format version {0}")]
    UnsupportedVersion(u8),
    #[error("sealed record is malformed or truncated")]
    MalformedRecord,
    #[error("plaintext exceeds the maximum record size")]
    RecordTooLarge,
    #[error("public key is invalid (low-order point)")]
    InvalidPublicKey,
    #[error("passphrase must be at least {0} characters")]
    WeakPassphrase(usize),
    #[error("key derivation parameters are outside the accepted range")]
    BadKdfParams,
    #[error("operating-system secret protection failed: {0}")]
    Protector(String),
    #[error("no operating-system secret protector is available on this platform")]
    ProtectorUnavailable,
    #[error("refusing to overwrite existing file: {0}")]
    WouldOverwrite(String),
    #[error("random number generator failure")]
    Rng,
    #[error("internal self-check failed: {0}")]
    SelfCheck(&'static str),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CryptoError>;
