//! Hermes cryptography: platform-independent key splitting, record encryption and share files.
//! Windows-specific protection (DPAPI) is injected through [`protector::SecretProtector`].
#![forbid(unsafe_code)]

pub mod error;
mod fsutil;
mod hexutil;
pub mod keys;
pub mod legacy;
pub mod protector;
pub mod provision;
pub mod record;
pub mod seal;
pub mod shares;
pub mod sign;

#[cfg(test)]
mod tests;

pub use error::{CryptoError, Result};
pub use keys::{
    generate_key_set, reconstruct_private_key, KeyId, KeySet, PrivateKey, PublicKeyInfo,
    SecretShare,
};
pub use protector::SecretProtector;
pub use record::Record;
pub use seal::{Opener, RecordContext, Sealer};
pub use sign::{CheckpointBody, RecordSigner};
