//! Public-key record encryption with HPKE (RFC 9180), via the `hpke` crate.
//!
//! Cipher suite (fixed, no negotiation):
//!   mode  = 0x00 Base
//!   KEM   = 0x0020 DHKEM(X25519, HKDF-SHA256)
//!   KDF   = 0x0001 HKDF-SHA256
//!   AEAD  = 0x0003 ChaCha20-Poly1305   (RFC 9180 defines no XChaCha20 id; see docs/HPKE.md)
//!
//! Every record is an independent single-shot HPKE message: fresh encapsulation (ephemeral X25519
//! key) => fresh AEAD key and nonce per record, so nonce reuse is structurally impossible.
//! The collector needs only the recipient *public* key.
//!
//! Wire format v2:  version(1)=2 | enc(32) | ciphertext+tag(16)
//! (the key id is *not* inside the blob: it is the `key_id` column and is bound through the
//! HPKE `info` and `aad`, so a wrong/changed key id simply fails authentication.)
//!
//! HPKE info = "hermes/record/v2 info" | key_id
//! HPKE aad  = "hermes/record/v2 aad"  | key_id | seq | ts_us      (the only plaintext row metadata)

use crate::error::{CryptoError, Result};
use crate::keys::{KeyId, PrivateKey, PublicKeyInfo};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as KemTrait, OpModeR, OpModeS, Serializable};
use zeroize::Zeroizing;

type K = X25519HkdfSha256;
type Kd = HkdfSha256;
type A = ChaCha20Poly1305;

pub const FORMAT_VERSION: u8 = 2;
pub const ENC_LEN: usize = 32;
pub const TAG_LEN: usize = 16;
pub const MIN_SEALED_LEN: usize = 1 + ENC_LEN + TAG_LEN;
/// Upper bound on the (padded) plaintext handed to the AEAD.
pub const MAX_PLAINTEXT: usize = 16 * 1024 * 1024 + 4096;

/// The plaintext row metadata authenticated together with each record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordContext {
    pub seq: u64,
    pub ts_us: i64,
}

fn info(key_id: &KeyId) -> Vec<u8> {
    let mut v = b"hermes/record/v2 info".to_vec();
    v.extend_from_slice(&key_id.0);
    v
}

fn aad(key_id: &KeyId, ctx: &RecordContext) -> Vec<u8> {
    let mut v = b"hermes/record/v2 aad".to_vec();
    v.extend_from_slice(&key_id.0);
    v.extend_from_slice(&ctx.seq.to_le_bytes());
    v.extend_from_slice(&ctx.ts_us.to_le_bytes());
    v
}

/// Encrypts records for one public key. Holds no secret material.
#[derive(Clone)]
pub struct Sealer {
    info: PublicKeyInfo,
    pk: <K as KemTrait>::PublicKey,
}

impl Sealer {
    pub fn new(info: PublicKeyInfo) -> Result<Self> {
        // Catches tampering with the key table (public key no longer matches its fingerprint).
        if KeyId::from_public(&info.public) != info.key_id {
            return Err(CryptoError::InvalidPublicKey);
        }
        let pk = <K as KemTrait>::PublicKey::from_bytes(&info.public)
            .map_err(|_| CryptoError::InvalidPublicKey)?;
        Ok(Sealer { info, pk })
    }

    pub fn key_id(&self) -> KeyId {
        self.info.key_id
    }

    /// Low-level: seals raw plaintext. Prefer [`Sealer::seal_record`].
    pub fn seal(&self, ctx: &RecordContext, plaintext: &[u8]) -> Result<Vec<u8>> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(CryptoError::RecordTooLarge);
        }
        let (enc, ct) = hpke::single_shot_seal::<A, Kd, K>(
            &OpModeS::Base,
            &self.pk,
            &info(&self.info.key_id),
            plaintext,
            &aad(&self.info.key_id, ctx),
        )
        // An all-zero (low-order) public key makes DHKEM's encap fail.
        .map_err(|_| CryptoError::InvalidPublicKey)?;
        let enc = enc.to_bytes();
        let mut out = Vec::with_capacity(1 + ENC_LEN + ct.len());
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&enc);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Seals an activity record: kind, session id and body are encrypted and length-padded.
    pub fn seal_record(
        &self,
        ctx: &RecordContext,
        kind: u16,
        session_id: u64,
        body: &[u8],
    ) -> Result<Vec<u8>> {
        let plain = crate::record::encode(kind, session_id, body)?;
        self.seal(ctx, &plain)
    }
}

/// Decrypts records for one reconstructed private key (viewer side).
pub struct Opener {
    key: PrivateKey,
}

impl Opener {
    pub fn new(key: PrivateKey) -> Self {
        Opener { key }
    }
    pub fn key_id(&self) -> KeyId {
        self.key.info().key_id
    }

    /// Reads a LEGACY v1 record (pre-HPKE archives). Read-only compatibility path.
    pub fn open_legacy_v1(
        &self,
        ctx: &crate::legacy::LegacyContext,
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        crate::legacy::open_with_key(&self.key, ctx, sealed)
    }

    pub fn open(&self, ctx: &RecordContext, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if sealed.len() < MIN_SEALED_LEN {
            return Err(CryptoError::MalformedRecord);
        }
        if sealed[0] != FORMAT_VERSION {
            return Err(CryptoError::UnsupportedVersion(sealed[0]));
        }
        let raw = self.key.to_bytes();
        let sk = <K as KemTrait>::PrivateKey::from_bytes(&*raw)
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        let enc = <K as KemTrait>::EncappedKey::from_bytes(&sealed[1..1 + ENC_LEN])
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        let id = self.key_id();
        let pt = hpke::single_shot_open::<A, Kd, K>(
            &OpModeR::Base,
            &sk,
            &enc,
            &info(&id),
            &sealed[1 + ENC_LEN..],
            &aad(&id, ctx),
        )
        .map_err(|_| CryptoError::AuthenticationFailed)?;
        Ok(Zeroizing::new(pt))
    }

    pub fn open_record(&self, ctx: &RecordContext, sealed: &[u8]) -> Result<crate::record::Record> {
        let plain = self.open(ctx, sealed)?;
        crate::record::decode(&plain)
    }
}
