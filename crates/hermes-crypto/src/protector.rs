//! Abstraction over OS-bound secret protection (Windows DPAPI in production).
//!
//! There is deliberately NO plaintext implementation of this trait. On platforms without a
//! protector the share-A operations fail with `ProtectorUnavailable`.

use crate::error::Result;
use zeroize::Zeroizing;

pub trait SecretProtector {
    /// Encrypts `plaintext` so that only this user/machine can decrypt it.
    /// `context` is mixed in as additional entropy/binding (e.g. key id); it is not secret.
    fn protect(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>>;
    fn unprotect(&self, blob: &[u8], context: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

/// Test-only protector: XChaCha20-Poly1305 under a random key held in memory.
/// Models "bound to this machine/user" for tests on Linux. Not OS-bound; never ship it.
#[cfg(any(test, feature = "test-support"))]
pub struct TestProtector {
    key: Zeroizing<[u8; 32]>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestProtector {
    pub fn new() -> Self {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).expect("rng");
        TestProtector {
            key: Zeroizing::new(k),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Default for TestProtector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl SecretProtector for TestProtector {
    fn protect(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>> {
        use crate::error::CryptoError;
        use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| CryptoError::Rng)?;
        let cipher = XChaCha20Poly1305::new(&(*self.key).into());
        let mut buf = plaintext.to_vec();
        cipher
            .encrypt_in_place(&XNonce::from(nonce), context, &mut buf)
            .map_err(|_| CryptoError::Protector("encrypt".into()))?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&buf);
        Ok(out)
    }
    fn unprotect(&self, blob: &[u8], context: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        use crate::error::CryptoError;
        use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
        if blob.len() < 24 + 16 {
            return Err(CryptoError::Protector("blob too short".into()));
        }
        let (n, ct) = blob.split_at(24);
        let nonce: [u8; 24] = n.try_into().unwrap();
        let cipher = XChaCha20Poly1305::new(&(*self.key).into());
        let mut buf = Zeroizing::new(ct.to_vec());
        cipher
            .decrypt_in_place(&XNonce::from(nonce), context, &mut *buf)
            .map_err(|_| {
                CryptoError::Protector("decrypt (wrong user/machine or tampered)".into())
            })?;
        Ok(buf)
    }
}
