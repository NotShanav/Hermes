//! DPAPI (`CryptProtectData` / `CryptUnprotectData`), current-user scope.
//!
//! * Scope: the data can only be decrypted by the same Windows user account (on this
//!   machine, or any machine where that roaming profile's DPAPI master key is available).
//!   `CRYPTPROTECT_LOCAL_MACHINE` is deliberately NOT used.
//! * `context` is passed as optional entropy so a Share-A blob copied between key ids
//!   or onto other Hermes data fails to unprotect.
//! * `CRYPTPROTECT_UI_FORBIDDEN`: the collector must never pop up UI.
//! * Plain-text output buffers from the OS are zeroed before being freed.
//!
//! This file is the ONLY place in the workspace containing `unsafe`. It compiles only on
//! Windows and has NOT yet been run by the author of this milestone (see docs/STATUS.md).

#![allow(unsafe_code)]

use hermes_crypto::{CryptoError, Result, SecretProtector};
use windows::core::w;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::{Zeroize, Zeroizing};

pub struct DpapiProtector;

impl DpapiProtector {
    pub fn new() -> Self {
        DpapiProtector
    }
}

impl Default for DpapiProtector {
    fn default() -> Self {
        Self::new()
    }
}

fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
    let len =
        u32::try_from(data.len()).map_err(|_| CryptoError::Protector("input too large".into()))?;
    Ok(CRYPT_INTEGER_BLOB {
        cbData: len,
        pbData: data.as_ptr() as *mut u8,
    })
}

/// Entropy = fixed application tag || caller context.
fn entropy(context: &[u8]) -> Vec<u8> {
    let mut e = b"hermes/dpapi/v1".to_vec();
    e.extend_from_slice(context);
    e
}

/// Copies the OS-allocated output into a Vec, zeroes the OS buffer, frees it.
///
/// # Safety
/// `out` must have been filled in by a successful DPAPI call (allocated with LocalAlloc).
unsafe fn take_output(out: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
    if out.pbData.is_null() {
        return Zeroizing::new(Vec::new());
    }
    let slice = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
    let copy = Zeroizing::new(slice.to_vec());
    slice.zeroize();
    let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
    copy
}

impl SecretProtector for DpapiProtector {
    fn protect(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>> {
        let ent = entropy(context);
        let input = blob(plaintext)?;
        let ent_blob = blob(&ent)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: all pointers reference live buffers for the duration of the call; `out` is
        // filled by the OS and released in `take_output`.
        unsafe {
            CryptProtectData(
                &input,
                w!("Hermes share A"),
                Some(&ent_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| CryptoError::Protector(format!("CryptProtectData: {e}")))?;
            // The protected blob is not secret plaintext; copy out of the zeroizing wrapper.
            Ok(take_output(out).to_vec())
        }
    }

    fn unprotect(&self, blob_in: &[u8], context: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let ent = entropy(context);
        let input = blob(blob_in)?;
        let ent_blob = blob(&ent)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&ent_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| CryptoError::Protector(format!("CryptUnprotectData: {e}")))?;
            Ok(take_output(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = DpapiProtector::new();
        let blob = p.protect(b"share-a-bytes", b"ctx").unwrap();
        assert_ne!(&blob[..], b"share-a-bytes");
        assert_eq!(&**p.unprotect(&blob, b"ctx").unwrap(), b"share-a-bytes");
    }

    #[test]
    fn wrong_context_fails() {
        let p = DpapiProtector::new();
        let blob = p.protect(b"x", b"ctx-1").unwrap();
        assert!(p.unprotect(&blob, b"ctx-2").is_err());
    }

    #[test]
    fn tampered_blob_fails() {
        let p = DpapiProtector::new();
        let mut blob = p.protect(b"secret", b"c").unwrap();
        let n = blob.len();
        blob[n / 2] ^= 1;
        assert!(p.unprotect(&blob, b"c").is_err());
    }

    #[test]
    fn empty_plaintext_ok() {
        let p = DpapiProtector::new();
        let blob = p.protect(b"", b"c").unwrap();
        assert_eq!(p.unprotect(&blob, b"c").unwrap().len(), 0);
    }
}
