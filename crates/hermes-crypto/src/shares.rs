//! On-disk formats for the two shares, plus the passphrase-protected Share-A backup.
//!
//! * Share A (local): binary file, payload protected by the OS (`SecretProtector`/DPAPI).
//! * Share B (USB):   small text file, hex, with a corruption-detecting checksum. The share
//!   itself is NOT encrypted: by design it is useless without Share A.
//! * Share A backup:  text file, Argon2id + XChaCha20-Poly1305, for moving to a new PC.

use crate::error::{CryptoError, Result};
use crate::fsutil::atomic_write;
use crate::hexutil;
use crate::keys::{KeyId, SecretShare, SHARE_LEN};
use crate::protector::SecretProtector;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

// ---------------------------------------------------------------- Share A (local, DPAPI)

const A_MAGIC: &[u8; 8] = b"HRMSA1\0\0";

pub fn share_a_path(dir: &Path, key_id: &KeyId) -> PathBuf {
    dir.join(format!("{}.sharea", key_id.to_hex()))
}

fn a_context(key_id: &KeyId) -> Vec<u8> {
    let mut c = b"hermes/share-a/v1".to_vec();
    c.extend_from_slice(&key_id.0);
    c
}

pub fn write_share_a(
    path: &Path,
    key_id: &KeyId,
    share: &SecretShare,
    protector: &dyn SecretProtector,
    overwrite: bool,
) -> Result<()> {
    let blob = protector.protect(share.as_bytes(), &a_context(key_id))?;
    let mut f = Vec::with_capacity(8 + 16 + 4 + blob.len());
    f.extend_from_slice(A_MAGIC);
    f.extend_from_slice(&key_id.0);
    f.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    f.extend_from_slice(&blob);
    atomic_write(path, &f, overwrite)
}

pub fn read_share_a(
    path: &Path,
    expected: &KeyId,
    protector: &dyn SecretProtector,
) -> Result<SecretShare> {
    let f = std::fs::read(path)?;
    if f.len() < 28 || &f[..8] != A_MAGIC {
        return Err(CryptoError::MalformedFile("not a Hermes share-A file"));
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&f[8..24]);
    let id = KeyId(id);
    if &id != expected {
        return Err(CryptoError::KeyIdMismatch {
            expected: expected.to_hex(),
            found: id.to_hex(),
        });
    }
    let len = u32::from_le_bytes(f[24..28].try_into().unwrap()) as usize;
    if f.len() != 28 + len {
        return Err(CryptoError::MalformedFile("length mismatch"));
    }
    let plain = protector.unprotect(&f[28..], &a_context(&id))?;
    SecretShare::from_bytes(plain.to_vec())
}

// ---------------------------------------------------------------- signing key (local, DPAPI)

const S_MAGIC: &[u8; 8] = b"HRMSS1\0\0";

pub fn signing_key_path(dir: &Path, key_id: &KeyId) -> PathBuf {
    dir.join(format!("{}.signkey", key_id.to_hex()))
}

fn s_context(key_id: &KeyId) -> Vec<u8> {
    let mut c = b"hermes/signing-key/v1".to_vec();
    c.extend_from_slice(&key_id.0);
    c
}

pub fn write_signing_seed(
    path: &Path,
    key_id: &KeyId,
    seed: &[u8; 32],
    protector: &dyn SecretProtector,
    overwrite: bool,
) -> Result<()> {
    let blob = protector.protect(seed, &s_context(key_id))?;
    let mut f = Vec::with_capacity(8 + 16 + 4 + blob.len());
    f.extend_from_slice(S_MAGIC);
    f.extend_from_slice(&key_id.0);
    f.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    f.extend_from_slice(&blob);
    atomic_write(path, &f, overwrite)
}

pub fn read_signing_seed(
    path: &Path,
    expected: &KeyId,
    protector: &dyn SecretProtector,
) -> Result<Zeroizing<[u8; 32]>> {
    let f = std::fs::read(path)?;
    if f.len() < 28 || &f[..8] != S_MAGIC {
        return Err(CryptoError::MalformedFile("not a Hermes signing-key file"));
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&f[8..24]);
    if &KeyId(id) != expected {
        return Err(CryptoError::KeyIdMismatch {
            expected: expected.to_hex(),
            found: KeyId(id).to_hex(),
        });
    }
    let len = u32::from_le_bytes(f[24..28].try_into().unwrap()) as usize;
    if f.len() != 28 + len {
        return Err(CryptoError::MalformedFile("length mismatch"));
    }
    let plain = protector.unprotect(&f[28..], &s_context(expected))?;
    let arr: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::MalformedFile("bad seed length"))?;
    Ok(Zeroizing::new(arr))
}

// ---------------------------------------------------------------- Share B (USB, text)

const B_HEADER_V1: &str = "HERMES-SHARE-B v1";
const B_HEADER_V2: &str = "HERMES-SHARE-B v2";

/// Parsed USB key file. `sign_public` is the trust anchor for record signatures; it is absent in
/// v1 files written by the previous milestone (those key sets have no signing key).
pub struct ShareBFile {
    pub key_id: KeyId,
    pub share: SecretShare,
    pub sign_public: Option<[u8; 32]>,
}

fn b_check(header: &str, key_id_hex: &str, share_hex: &str, sign_hex: Option<&str>) -> String {
    let mut h = Sha256::new();
    if header == B_HEADER_V1 {
        h.update(b"hermes/share-file/v1\nB\n");
    } else {
        h.update(b"hermes/share-file/v2\nB\n");
    }
    h.update(key_id_hex.as_bytes());
    h.update(b"\n");
    h.update(share_hex.as_bytes());
    if let Some(sg) = sign_hex {
        h.update(b"\n");
        h.update(sg.as_bytes());
    }
    hexutil::encode(&h.finalize()[..8])
}

/// Writes the current (v2) format.
pub fn encode_share_b(
    key_id: &KeyId,
    share: &SecretShare,
    sign_public: &[u8; 32],
) -> Zeroizing<String> {
    let kid = key_id.to_hex();
    let sh = Zeroizing::new(hexutil::encode(share.as_bytes()));
    let sg = hexutil::encode(sign_public);
    Zeroizing::new(format!(
        "{B_HEADER_V2}\nkey_id: {kid}\nshare: {}\nsign_pub: {sg}\ncheck: {}\n",
        *sh,
        b_check(B_HEADER_V2, &kid, &sh, Some(&sg))
    ))
}

/// Legacy v1 encoder: only for migration fixtures/tests.
#[cfg(any(test, feature = "test-support"))]
pub fn encode_share_b_v1(key_id: &KeyId, share: &SecretShare) -> Zeroizing<String> {
    let kid = key_id.to_hex();
    let sh = Zeroizing::new(hexutil::encode(share.as_bytes()));
    Zeroizing::new(format!(
        "{B_HEADER_V1}\nkey_id: {kid}\nshare: {}\ncheck: {}\n",
        *sh,
        b_check(B_HEADER_V1, &kid, &sh, None)
    ))
}

pub fn write_share_b(
    path: &Path,
    key_id: &KeyId,
    share: &SecretShare,
    sign_public: &[u8; 32],
    overwrite: bool,
) -> Result<()> {
    let text = encode_share_b(key_id, share, sign_public);
    atomic_write(path, text.as_bytes(), overwrite)
}

pub fn parse_share_b(text: &str) -> Result<ShareBFile> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let header = match lines.next() {
        Some(B_HEADER_V1) => B_HEADER_V1,
        Some(B_HEADER_V2) => B_HEADER_V2,
        _ => {
            return Err(CryptoError::MalformedFile(
                "missing or unsupported Share-B header",
            ))
        }
    };
    let field = |l: Option<&str>, name: &'static str| -> Result<String> {
        let l = l.ok_or(CryptoError::MalformedFile("truncated share file"))?;
        l.strip_prefix(name)
            .and_then(|r| r.strip_prefix(": "))
            .map(|s| s.to_string())
            .ok_or(CryptoError::MalformedFile("unexpected field order"))
    };
    let kid = field(lines.next(), "key_id")?;
    let sh = Zeroizing::new(field(lines.next(), "share")?);
    let sg = if header == B_HEADER_V2 {
        Some(field(lines.next(), "sign_pub")?)
    } else {
        None
    };
    let chk = field(lines.next(), "check")?;
    if lines.any(|l| !l.is_empty()) {
        return Err(CryptoError::MalformedFile("trailing data"));
    }
    if chk != b_check(header, &kid, &sh, sg.as_deref()) {
        return Err(CryptoError::ChecksumMismatch);
    }
    let key_id = KeyId::from_hex(&kid).ok_or(CryptoError::MalformedFile("bad key_id"))?;
    let bytes = hexutil::decode(&sh).ok_or(CryptoError::MalformedFile("bad share hex"))?;
    if bytes.len() != SHARE_LEN {
        return Err(CryptoError::MalformedShare("wrong length"));
    }
    let sign_public = match sg {
        None => None,
        Some(h) => {
            let v = hexutil::decode(&h).ok_or(CryptoError::MalformedFile("bad sign_pub hex"))?;
            Some(
                <[u8; 32]>::try_from(v)
                    .map_err(|_| CryptoError::MalformedFile("bad sign_pub length"))?,
            )
        }
    };
    Ok(ShareBFile {
        key_id,
        share: SecretShare::from_bytes(bytes)?,
        sign_public,
    })
}

pub fn read_share_b_file(path: &Path, expected: &KeyId) -> Result<ShareBFile> {
    let text = Zeroizing::new(std::fs::read_to_string(path)?);
    let f = parse_share_b(&text)?;
    if &f.key_id != expected {
        return Err(CryptoError::KeyIdMismatch {
            expected: expected.to_hex(),
            found: f.key_id.to_hex(),
        });
    }
    Ok(f)
}

pub fn read_share_b(path: &Path, expected: &KeyId) -> Result<SecretShare> {
    Ok(read_share_b_file(path, expected)?.share)
}

/// Reads only the key id from a Share-B file (to pick the right key without a prior guess).
pub fn peek_share_b_key_id(path: &Path) -> Result<KeyId> {
    let text = Zeroizing::new(std::fs::read_to_string(path)?);
    Ok(parse_share_b(&text)?.key_id)
}

// ---------------------------------------------------------------- Share A backup (passphrase)

pub const MIN_PASSPHRASE_CHARS: usize = 12;
const BK_HEADER: &str = "HERMES-SHARE-A-BACKUP v1";

#[derive(Clone, Copy, Debug)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}
impl KdfParams {
    /// Production default: 64 MiB, 3 passes, 1 lane (backup/restore are rare operations).
    pub const STRONG: KdfParams = KdfParams {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// Fast parameters for unit tests only.
    pub const TEST_ONLY: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    fn validate(&self) -> Result<()> {
        // Bounds protect against absurd values in a hostile backup file (memory DoS) and
        // against silently weak values.
        if self.m_kib > 1024 * 1024 || self.t == 0 || self.t > 20 || self.p == 0 || self.p > 8 {
            return Err(CryptoError::BadKdfParams);
        }
        Ok(())
    }
}

fn derive_backup_key(pass: &str, salt: &[u8], kdf: KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    kdf.validate()?;
    let params =
        Params::new(kdf.m_kib, kdf.t, kdf.p, Some(32)).map_err(|_| CryptoError::BadKdfParams)?;
    let a2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    a2.hash_password_into(pass.as_bytes(), salt, &mut *out)
        .map_err(|_| CryptoError::BadKdfParams)?;
    Ok(out)
}

fn backup_aad(key_id: &KeyId, kdf: KdfParams) -> Vec<u8> {
    let mut a = b"hermes/share-a-backup/v1".to_vec();
    a.extend_from_slice(&key_id.0);
    a.extend_from_slice(&kdf.m_kib.to_le_bytes());
    a.extend_from_slice(&kdf.t.to_le_bytes());
    a.extend_from_slice(&kdf.p.to_le_bytes());
    a
}

pub fn export_share_a_backup(
    key_id: &KeyId,
    share: &SecretShare,
    passphrase: &str,
    kdf: KdfParams,
) -> Result<String> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(CryptoError::WeakPassphrase(MIN_PASSPHRASE_CHARS));
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut salt).map_err(|_| CryptoError::Rng)?;
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::Rng)?;
    let key = derive_backup_key(passphrase, &salt, kdf)?;
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    let mut buf = Zeroizing::new(share.as_bytes().to_vec());
    cipher
        .encrypt_in_place(&XNonce::from(nonce), &backup_aad(key_id, kdf), &mut *buf)
        .map_err(|_| CryptoError::SelfCheck("backup encrypt"))?;
    Ok(format!(
        "{BK_HEADER}\nkey_id: {}\nkdf: argon2id m={} t={} p={}\nsalt: {}\nnonce: {}\ndata: {}\n",
        key_id.to_hex(),
        kdf.m_kib,
        kdf.t,
        kdf.p,
        hexutil::encode(&salt),
        hexutil::encode(&nonce),
        hexutil::encode(&buf)
    ))
}

pub fn import_share_a_backup(text: &str, passphrase: &str) -> Result<(KeyId, SecretShare)> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    if lines.next() != Some(BK_HEADER) {
        return Err(CryptoError::MalformedFile(
            "missing or unsupported backup header",
        ));
    }
    let mut take = |name: &'static str| -> Result<String> {
        let l = lines
            .next()
            .ok_or(CryptoError::MalformedFile("truncated backup"))?;
        l.strip_prefix(name)
            .and_then(|r| r.strip_prefix(": "))
            .map(|s| s.to_string())
            .ok_or(CryptoError::MalformedFile("unexpected field order"))
    };
    let key_id =
        KeyId::from_hex(&take("key_id")?).ok_or(CryptoError::MalformedFile("bad key_id"))?;
    let kdf_line = take("kdf")?;
    let salt = hexutil::decode(&take("salt")?).ok_or(CryptoError::MalformedFile("bad salt"))?;
    let nonce: [u8; 24] = hexutil::decode(&take("nonce")?)
        .and_then(|v| v.try_into().ok())
        .ok_or(CryptoError::MalformedFile("bad nonce"))?;
    let data = hexutil::decode(&take("data")?).ok_or(CryptoError::MalformedFile("bad data"))?;

    let mut kdf = None;
    if let Some(rest) = kdf_line.strip_prefix("argon2id ") {
        let mut m = None;
        let mut t = None;
        let mut p = None;
        for part in rest.split(' ') {
            match part.split_once('=') {
                Some(("m", v)) => m = v.parse::<u32>().ok(),
                Some(("t", v)) => t = v.parse::<u32>().ok(),
                Some(("p", v)) => p = v.parse::<u32>().ok(),
                _ => {}
            }
        }
        if let (Some(m_kib), Some(t), Some(p)) = (m, t, p) {
            kdf = Some(KdfParams { m_kib, t, p });
        }
    }
    let kdf = kdf.ok_or(CryptoError::MalformedFile("bad kdf line"))?;
    kdf.validate()?;
    if salt.len() < 16 {
        return Err(CryptoError::MalformedFile("salt too short"));
    }

    let key = derive_backup_key(passphrase, &salt, kdf)?;
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    let mut buf = Zeroizing::new(data);
    cipher
        .decrypt_in_place(&XNonce::from(nonce), &backup_aad(&key_id, kdf), &mut *buf)
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    Ok((key_id, SecretShare::from_bytes(buf.to_vec())?))
}
