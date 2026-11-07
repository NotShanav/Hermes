//! Record authenticity: hash chain + Ed25519-signed checkpoints.
//!
//! Why: encrypting to a public key proves nothing about who wrote a record — anyone holding the
//! (non-secret) public key can create one. The collector therefore also holds an Ed25519 *signing*
//! key (DPAPI-protected, never in the database). Each flush appends rows to a hash chain and the
//! collector signs a checkpoint over the chain head. The viewer verifies checkpoints against the
//! verifying key stored in the USB share file (Share B), which an attacker who only has the PC's
//! disk cannot alter.
//!
//! chain_i = SHA256("hermes/chain/v1" | chain_{i-1} | seq | ts_us | key_id | SHA256(payload))
//! checkpoint message = "hermes/checkpoint/v1" | signer_id | first_seq | last_seq | prev_chain | last_chain
//!
//! Detects: modified/reordered/inserted/deleted rows inside signed ranges; forged records by
//! parties without the signing key; swapped DB verifying key. Does NOT detect (see docs/SECURITY.md):
//! truncation to an older checkpoint, rollback to an old DB copy, or forgery by malware that runs
//! as the same Windows user and can unprotect the signing key.

use crate::error::{CryptoError, Result};
use crate::keys::KeyId;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const ZERO_CHAIN: [u8; 32] = [0u8; 32];

/// Fingerprint of a verifying key.
pub fn signer_id(verify_public: &[u8; 32]) -> [u8; 16] {
    let mut h = Sha256::new();
    h.update(b"hermes/signer-id/v1");
    h.update(verify_public);
    let d = h.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&d[..16]);
    id
}

pub fn chain_next(
    prev: &[u8; 32],
    seq: u64,
    ts_us: i64,
    key_id: &KeyId,
    payload: &[u8],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"hermes/chain/v1");
    h.update(prev);
    h.update(seq.to_le_bytes());
    h.update(ts_us.to_le_bytes());
    h.update(key_id.0);
    h.update(Sha256::digest(payload));
    h.finalize().into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointBody {
    pub signer_id: [u8; 16],
    pub first_seq: u64,
    pub last_seq: u64,
    pub prev_chain: [u8; 32],
    pub last_chain: [u8; 32],
}

impl CheckpointBody {
    fn message(&self) -> Vec<u8> {
        let mut m = b"hermes/checkpoint/v1".to_vec();
        m.extend_from_slice(&self.signer_id);
        m.extend_from_slice(&self.first_seq.to_le_bytes());
        m.extend_from_slice(&self.last_seq.to_le_bytes());
        m.extend_from_slice(&self.prev_chain);
        m.extend_from_slice(&self.last_chain);
        m
    }
}

/// Collector-side signing capability (secret). Zeroized on drop by ed25519-dalek.
pub struct RecordSigner {
    key: SigningKey,
    public: [u8; 32],
}

impl RecordSigner {
    pub fn generate() -> Result<(RecordSigner, Zeroizing<[u8; 32]>)> {
        let mut seed = Zeroizing::new([0u8; 32]);
        getrandom::fill(&mut *seed).map_err(|_| CryptoError::Rng)?;
        let s = Self::from_seed(&seed);
        Ok((s, seed))
    }
    pub fn from_seed(seed: &[u8; 32]) -> RecordSigner {
        let key = SigningKey::from_bytes(seed);
        let public = key.verifying_key().to_bytes();
        RecordSigner { key, public }
    }
    pub fn public(&self) -> [u8; 32] {
        self.public
    }
    pub fn signer_id(&self) -> [u8; 16] {
        signer_id(&self.public)
    }
    pub fn sign_checkpoint(&self, body: &CheckpointBody) -> [u8; 64] {
        self.key.sign(&body.message()).to_bytes()
    }
}

/// Viewer-side verification (public).
pub fn verify_checkpoint(
    verify_public: &[u8; 32],
    body: &CheckpointBody,
    sig: &[u8; 64],
) -> Result<()> {
    if body.signer_id != signer_id(verify_public) {
        return Err(CryptoError::AuthenticationFailed);
    }
    let vk = VerifyingKey::from_bytes(verify_public).map_err(|_| CryptoError::InvalidPublicKey)?;
    let sig = Signature::from_bytes(sig);
    vk.verify_strict(&body.message(), &sig)
        .map_err(|_| CryptoError::AuthenticationFailed)
}
