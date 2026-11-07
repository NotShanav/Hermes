//! Windows-specific secret protection (DPAPI) behind `hermes_crypto::SecretProtector`.
//!
//! On non-Windows targets `default_protector()` returns `ProtectorUnavailable`: there is
//! intentionally no plaintext fallback.

use hermes_crypto::{CryptoError, SecretProtector};

#[cfg(windows)]
mod dpapi;
#[cfg(windows)]
mod fsinfo;
pub mod single;
#[cfg(windows)]
pub use dpapi::DpapiProtector;

/// The protector Hermes uses on this platform.
pub fn default_protector() -> Result<Box<dyn SecretProtector>, CryptoError> {
    #[cfg(windows)]
    {
        Ok(Box::new(DpapiProtector::new()))
    }
    #[cfg(not(windows))]
    {
        Err(CryptoError::ProtectorUnavailable)
    }
}

/// Description of the volume a path lives on (for the filesystem validation tool).
#[derive(Debug, Clone)]
pub struct FsInfo {
    pub volume_root: String,
    pub fs_name: String,
    pub drive_type: String,
    pub max_component_len: u32,
}

/// Windows: volume root, filesystem name (NTFS/exFAT/FAT32/...), drive type. Elsewhere: None.
pub fn fs_info(path: &std::path::Path) -> Option<FsInfo> {
    #[cfg(windows)]
    {
        fsinfo::fs_info(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}
