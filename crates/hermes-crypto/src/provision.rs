//! Key lifecycle orchestration: creation, verification, loading (viewer), restore from backup.
//! Rotation = `create_key_set` again + registering the new public key as active (see hermes-db);
//! old key sets stay on disk/USB so historical records remain readable.

use crate::error::Result;
use crate::keys::{generate_key_set, reconstruct_private_key, KeyId, PrivateKey, PublicKeyInfo};
use crate::protector::SecretProtector;
use crate::shares::*;
use std::path::Path;

/// Public results of key-set creation: everything the collector/DB may store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewKey {
    pub info: PublicKeyInfo,
    /// Ed25519 verifying key for record signatures (also written into Share B).
    pub sign_public: [u8; 32],
}

/// Creates a new key set: Share A + signing seed -> `local_dir` (OS-protected), Share B -> `share_b_path`.
/// Succeeds only if everything was written *and* read back and reconstructs the same keys.
/// The private key exists only transiently inside this call.
pub fn create_key_set(
    local_dir: &Path,
    share_b_path: &Path,
    protector: &dyn SecretProtector,
) -> Result<NewKey> {
    use crate::error::CryptoError;
    use crate::sign::RecordSigner;
    if share_b_path.exists() {
        return Err(CryptoError::WouldOverwrite(
            share_b_path.display().to_string(),
        ));
    }
    let ks = generate_key_set()?;
    let (signer, seed) = RecordSigner::generate()?;
    let sign_public = signer.public();
    let a_path = share_a_path(local_dir, &ks.info.key_id);
    let s_path = signing_key_path(local_dir, &ks.info.key_id);
    let rollback = |b: bool| {
        let _ = std::fs::remove_file(&a_path);
        let _ = std::fs::remove_file(&s_path);
        if b {
            let _ = std::fs::remove_file(share_b_path);
        }
    };
    write_share_a(&a_path, &ks.info.key_id, &ks.share_a, protector, false)?;
    if let Err(e) = write_signing_seed(&s_path, &ks.info.key_id, &seed, protector, false) {
        rollback(false);
        return Err(e);
    }
    if let Err(e) = write_share_b(
        share_b_path,
        &ks.info.key_id,
        &ks.share_b,
        &sign_public,
        false,
    ) {
        rollback(false);
        return Err(e);
    }
    // Read-back verification (catches a flaky USB drive before any data depends on the key).
    let check = (|| -> Result<()> {
        let (k, vk) = load_viewer_material(local_dir, &ks.info.key_id, share_b_path, protector)?;
        if k.info() != ks.info || vk != Some(sign_public) {
            return Err(CryptoError::SelfCheck("read-back key mismatch"));
        }
        if load_signer(local_dir, &ks.info.key_id, protector)?.public() != sign_public {
            return Err(CryptoError::SelfCheck("read-back signing key mismatch"));
        }
        Ok(())
    })();
    match check {
        Ok(()) => Ok(NewKey {
            info: ks.info,
            sign_public,
        }),
        Err(e) => {
            rollback(true);
            Err(e)
        }
    }
}

/// Collector path: loads the signing key (never the decryption shares).
pub fn load_signer(
    local_dir: &Path,
    key_id: &KeyId,
    protector: &dyn SecretProtector,
) -> Result<crate::sign::RecordSigner> {
    let seed = read_signing_seed(&signing_key_path(local_dir, key_id), key_id, protector)?;
    Ok(crate::sign::RecordSigner::from_seed(&seed))
}

/// Viewer path: read both shares, reconstruct in memory, verify fingerprint. Also returns the
/// signature trust anchor from the USB file (None for v1 key sets).
/// The returned key zeroizes on drop. Share bytes read here are zeroized before return.
pub fn load_viewer_material(
    local_dir: &Path,
    key_id: &KeyId,
    share_b_path: &Path,
    protector: &dyn SecretProtector,
) -> Result<(PrivateKey, Option<[u8; 32]>)> {
    let a = read_share_a(&share_a_path(local_dir, key_id), key_id, protector)?;
    let b = read_share_b_file(share_b_path, key_id)?;
    let key = reconstruct_private_key(&a, &b.share, key_id)?;
    Ok((key, b.sign_public))
}

/// Same as [`load_viewer_material`] but only the private key.
pub fn load_private_key(
    local_dir: &Path,
    key_id: &KeyId,
    share_b_path: &Path,
    protector: &dyn SecretProtector,
) -> Result<PrivateKey> {
    Ok(load_viewer_material(local_dir, key_id, share_b_path, protector)?.0)
}

/// Verifies a Share-B file (e.g. a freshly copied backup USB) against the local Share A
/// without keeping the key.
pub fn verify_key_set(
    local_dir: &Path,
    key_id: &KeyId,
    share_b_path: &Path,
    protector: &dyn SecretProtector,
) -> Result<()> {
    load_private_key(local_dir, key_id, share_b_path, protector).map(|_| ())
}

/// Disaster recovery on a new PC / new Windows profile: restores Share A from the
/// passphrase-protected backup and re-protects it with this machine's protector.
pub fn restore_share_a(
    local_dir: &Path,
    backup_text: &str,
    passphrase: &str,
    protector: &dyn SecretProtector,
    overwrite: bool,
) -> Result<KeyId> {
    let (key_id, share) = import_share_a_backup(backup_text, passphrase)?;
    write_share_a(
        &share_a_path(local_dir, &key_id),
        &key_id,
        &share,
        protector,
        overwrite,
    )?;
    Ok(key_id)
}

/// Re-exports the local Share A as a passphrase-protected backup file text.
pub fn backup_share_a(
    local_dir: &Path,
    key_id: &KeyId,
    passphrase: &str,
    kdf: KdfParams,
    protector: &dyn SecretProtector,
) -> Result<String> {
    let a = read_share_a(&share_a_path(local_dir, key_id), key_id, protector)?;
    export_share_a_backup(key_id, &a, passphrase, kdf)
}
