//! LEGACY record format v1 (read-only). Superseded by HPKE (see seal.rs). Retained ONLY so that
//! archives written by Beta 0.1 milestone 2/4 stay readable. Never used to write new data
//! (sealing is compiled only for tests / the `test-support` feature, to build migration fixtures).
//!
//! Original description: public-key record encryption, built only from vetted primitives:
//! X25519 (ephemeral-static) -> HKDF-SHA256 -> XChaCha20-Poly1305.
//!
//! The collector holds only the public key, so it can encrypt without ever touching the
//! private key or either share. Each record uses a fresh ephemeral key and random 192-bit nonce.
//!
//! Wire format v1:  version(1) | key_id(16) | ephemeral_pub(32) | nonce(24) | ciphertext+tag
//!
//! Associated data = "hermes/record/v1" | header | seq | ts_us | session_id | kind, so a record
//! cannot be moved to another row, reordered, re-timestamped or re-typed without detection.

use crate::error::{CryptoError, Result};
#[cfg(any(test, feature = "test-support"))]
use crate::keys::PublicKeyInfo;
use crate::keys::{KeyId, PrivateKey};
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
#[cfg(any(test, feature = "test-support"))]
use x25519_dalek::EphemeralSecret;
use x25519_dalek::PublicKey;
use zeroize::Zeroizing;

pub const LEGACY_FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 1 + 16 + 32 + 24;
const TAG_LEN: usize = 16;
pub const LEGACY_MAX_PLAINTEXT: usize = 16 * 1024 * 1024;

/// Row-level context authenticated together with each record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyContext {
    pub seq: u64,
    pub ts_us: i64,
    pub session_id: i64,
    pub kind: u16,
}

fn aad(header: &[u8], ctx: &LegacyContext) -> Vec<u8> {
    let mut a = Vec::with_capacity(16 + header.len() + 26);
    a.extend_from_slice(b"hermes/record/v1");
    a.extend_from_slice(header);
    a.extend_from_slice(&ctx.seq.to_le_bytes());
    a.extend_from_slice(&ctx.ts_us.to_le_bytes());
    a.extend_from_slice(&ctx.session_id.to_le_bytes());
    a.extend_from_slice(&ctx.kind.to_le_bytes());
    a
}

fn derive_key(
    shared: &[u8; 32],
    eph_pub: &[u8; 32],
    recipient_pub: &[u8; 32],
) -> Result<Zeroizing<[u8; 32]>> {
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(eph_pub);
    salt[32..].copy_from_slice(recipient_pub);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut okm = Zeroizing::new([0u8; 32]);
    hk.expand(b"hermes/record/v1 xchacha20poly1305 key", &mut *okm)
        .map_err(|_| CryptoError::SelfCheck("hkdf expand"))?;
    Ok(okm)
}

/// Encrypts records for one public key. Holds no secret material.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub struct LegacySealer {
    info: PublicKeyInfo,
    public: PublicKey,
}

#[cfg(any(test, feature = "test-support"))]
impl LegacySealer {
    pub fn new(info: PublicKeyInfo) -> Result<Self> {
        // Reject keys whose fingerprint doesn't match (catches DB tampering of the key table).
        if KeyId::from_public(&info.public) != info.key_id {
            return Err(CryptoError::InvalidPublicKey);
        }
        Ok(LegacySealer {
            info,
            public: PublicKey::from(info.public),
        })
    }
    pub fn key_id(&self) -> KeyId {
        self.info.key_id
    }

    pub fn seal(&self, ctx: &LegacyContext, plaintext: &[u8]) -> Result<Vec<u8>> {
        if plaintext.len() > LEGACY_MAX_PLAINTEXT {
            return Err(CryptoError::RecordTooLarge);
        }
        let eph = EphemeralSecret::random();
        let eph_pub = PublicKey::from(&eph).to_bytes();
        let shared = eph.diffie_hellman(&self.public);
        if !shared.was_contributory() {
            return Err(CryptoError::InvalidPublicKey);
        }
        let key = derive_key(shared.as_bytes(), &eph_pub, &self.info.public)?;
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| CryptoError::Rng)?;

        let mut header = [0u8; HEADER_LEN];
        header[0] = LEGACY_FORMAT_VERSION;
        header[1..17].copy_from_slice(&self.info.key_id.0);
        header[17..49].copy_from_slice(&eph_pub);
        header[49..].copy_from_slice(&nonce);

        let cipher = XChaCha20Poly1305::new(&(*key).into());
        let mut ct = plaintext.to_vec();
        cipher
            .encrypt_in_place(&XNonce::from(nonce), &aad(&header, ctx), &mut ct)
            .map_err(|_| CryptoError::SelfCheck("aead encrypt"))?;
        let mut out = Vec::with_capacity(HEADER_LEN + ct.len());
        out.extend_from_slice(&header);
        out.extend_from_slice(&ct);
        Ok(out)
    }
}

/// Returns the key id a sealed record claims to be encrypted for (unauthenticated hint;
/// the AEAD verifies it implicitly because it feeds the AAD and the KDF salt).
pub fn legacy_sealed_key_id(sealed: &[u8]) -> Result<KeyId> {
    if sealed.len() < HEADER_LEN + TAG_LEN {
        return Err(CryptoError::MalformedRecord);
    }
    if sealed[0] != LEGACY_FORMAT_VERSION {
        return Err(CryptoError::UnsupportedVersion(sealed[0]));
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&sealed[1..17]);
    Ok(KeyId(id))
}

/// Decrypts a legacy v1 record with an already reconstructed private key.
pub fn open_with_key(
    key: &PrivateKey,
    ctx: &LegacyContext,
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let own = key.info().key_id;
    let id = legacy_sealed_key_id(sealed)?;
    if id != own {
        return Err(CryptoError::KeyIdMismatch {
            expected: own.to_hex(),
            found: id.to_hex(),
        });
    }
    let header = &sealed[..HEADER_LEN];
    let eph_pub: [u8; 32] = header[17..49].try_into().unwrap();
    let nonce: [u8; 24] = header[49..].try_into().unwrap();
    let shared = key.secret().diffie_hellman(&PublicKey::from(eph_pub));
    if !shared.was_contributory() {
        return Err(CryptoError::AuthenticationFailed);
    }
    let dk = derive_key(shared.as_bytes(), &eph_pub, &key.info().public)?;
    let cipher = XChaCha20Poly1305::new(&(*dk).into());
    let mut buf = Zeroizing::new(sealed[HEADER_LEN..].to_vec());
    cipher
        .decrypt_in_place(&XNonce::from(nonce), &aad(header, ctx), &mut *buf)
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    Ok(buf)
}
