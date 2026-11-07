//! Key generation, key identifiers, and 2-of-2 Shamir splitting of the private key.
//!
//! Construction: X25519 key pair. The 32-byte private scalar is split with Shamir secret
//! sharing (threshold 2, 2 shares, GF(256), via the `blahaj` crate). Either share alone is
//! information-theoretically independent of the key. Wrong or corrupted shares reconstruct
//! *garbage without any error from the library*, so every reconstruction is verified against
//! the key fingerprint (`KeyId`) before the key is used.

use crate::error::{CryptoError, Result};
use blahaj::{Share, Sharks};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub const KEY_ID_LEN: usize = 16;
/// 1 byte share index (x) + 32 bytes of share data.
pub const SHARE_LEN: usize = 33;

/// Fingerprint of a public key: first 16 bytes of SHA-256("hermes/key-id/v1" || public key).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(pub [u8; KEY_ID_LEN]);

impl KeyId {
    pub fn from_public(public: &[u8; 32]) -> Self {
        let mut h = Sha256::new();
        h.update(b"hermes/key-id/v1");
        h.update(public);
        let d = h.finalize();
        let mut id = [0u8; KEY_ID_LEN];
        id.copy_from_slice(&d[..KEY_ID_LEN]);
        KeyId(id)
    }
    pub fn to_hex(&self) -> String {
        crate::hexutil::encode(&self.0)
    }
    pub fn from_hex(s: &str) -> Option<Self> {
        let v = crate::hexutil::decode(s)?;
        let arr: [u8; KEY_ID_LEN] = v.try_into().ok()?;
        Some(KeyId(arr))
    }
}
impl std::fmt::Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}
impl std::fmt::Debug for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyId({})", self.to_hex())
    }
}

/// Public half of a key set: safe to store anywhere, used by the collector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PublicKeyInfo {
    pub key_id: KeyId,
    pub public: [u8; 32],
}

impl PublicKeyInfo {
    pub fn from_public(public: [u8; 32]) -> Self {
        PublicKeyInfo {
            key_id: KeyId::from_public(&public),
            public,
        }
    }
}

/// One share of the private key. Zeroized on drop; never printed.
pub struct SecretShare(Zeroizing<Vec<u8>>);

impl SecretShare {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let bytes = Zeroizing::new(bytes);
        if bytes.len() != SHARE_LEN {
            return Err(CryptoError::MalformedShare("wrong length"));
        }
        if bytes[0] == 0 {
            // x = 0 would be the secret itself in Shamir's scheme.
            return Err(CryptoError::MalformedShare("invalid share index"));
        }
        Ok(SecretShare(bytes))
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn index(&self) -> u8 {
        self.0[0]
    }
}
impl std::fmt::Debug for SecretShare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretShare(<redacted>)")
    }
}

/// The reconstructed private key. Lives only in memory; zeroized on drop
/// (x25519-dalek's `StaticSecret` is `ZeroizeOnDrop`). Intentionally not `Clone`.
pub struct PrivateKey {
    secret: StaticSecret,
    info: PublicKeyInfo,
}

impl PrivateKey {
    pub(crate) fn from_secret(secret: StaticSecret) -> Self {
        let pk = PublicKey::from(&secret);
        PrivateKey {
            info: PublicKeyInfo::from_public(pk.to_bytes()),
            secret,
        }
    }
    pub(crate) fn secret(&self) -> &StaticSecret {
        &self.secret
    }
    /// Raw private scalar (needed to build the HPKE private key). Zeroized on drop.
    pub(crate) fn to_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.secret.to_bytes())
    }
    pub fn info(&self) -> PublicKeyInfo {
        self.info
    }
}
impl std::fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PrivateKey({}, <redacted>)", self.info.key_id)
    }
}

/// Output of key generation. The private key never leaves `generate_key_set`.
pub struct KeySet {
    pub info: PublicKeyInfo,
    /// Stored on the Windows machine, protected by DPAPI.
    pub share_a: SecretShare,
    /// Stored on the USB drive.
    pub share_b: SecretShare,
}

/// Generates a fresh key pair and splits the private key into two shares.
/// Self-checks the split (reconstruction must reproduce the same public key).
pub fn generate_key_set() -> Result<KeySet> {
    let secret = StaticSecret::random();
    let public = PublicKey::from(&secret).to_bytes();
    let info = PublicKeyInfo::from_public(public);

    let raw = Zeroizing::new(secret.to_bytes());
    let sharks = Sharks(2);
    let mut dealer = sharks.dealer(&raw[..]);
    let s1 = dealer
        .next()
        .ok_or(CryptoError::SelfCheck("dealer produced no share"))?;
    let s2 = dealer
        .next()
        .ok_or(CryptoError::SelfCheck("dealer produced one share"))?;
    drop(dealer);
    let b1 = Zeroizing::new(Vec::<u8>::from(&s1));
    let b2 = Zeroizing::new(Vec::<u8>::from(&s2));
    drop((s1, s2));
    if b1[0] == b2[0] {
        return Err(CryptoError::SelfCheck(
            "dealer produced duplicate share indices",
        ));
    }
    let share_a = SecretShare::from_bytes(b1.to_vec())?;
    let share_b = SecretShare::from_bytes(b2.to_vec())?;

    // Self-check: reconstruction must give back the same key.
    let rebuilt = reconstruct_private_key(&share_a, &share_b, &info.key_id)?;
    if rebuilt.info() != info {
        return Err(CryptoError::SelfCheck("reconstruction mismatch"));
    }
    Ok(KeySet {
        info,
        share_a,
        share_b,
    })
}

/// Combines two shares into the private key and verifies it against `expected`.
/// Returns `WrongShares` if the result does not match the expected fingerprint.
pub fn reconstruct_private_key(
    a: &SecretShare,
    b: &SecretShare,
    expected: &KeyId,
) -> Result<PrivateKey> {
    if a.index() == b.index() {
        return Err(CryptoError::DuplicateShares);
    }
    let sa = Share::try_from(a.as_bytes())
        .map_err(|_| CryptoError::MalformedShare("unreadable share"))?;
    let sb = Share::try_from(b.as_bytes())
        .map_err(|_| CryptoError::MalformedShare("unreadable share"))?;
    let recovered = Sharks(2)
        .recover([&sa, &sb])
        .map_err(|_| CryptoError::WrongShares)?;
    let recovered = Zeroizing::new(recovered);
    let arr: Zeroizing<[u8; 32]> = Zeroizing::new(
        recovered
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::WrongShares)?,
    );
    let key = PrivateKey::from_secret(StaticSecret::from(*arr));
    if &key.info.key_id != expected {
        return Err(CryptoError::WrongShares);
    }
    Ok(key)
}
