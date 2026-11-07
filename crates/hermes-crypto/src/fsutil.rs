use crate::error::{CryptoError, Result};
use std::io::Write;
use std::path::Path;

/// Crash-safe write: temp file in the same directory, fsync, rename.
/// Fails if `overwrite` is false and the destination already exists.
pub fn atomic_write(path: &Path, data: &[u8], overwrite: bool) -> Result<()> {
    if !overwrite && path.exists() {
        return Err(CryptoError::WouldOverwrite(path.display().to_string()));
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut rnd = [0u8; 6];
    getrandom::fill(&mut rnd).map_err(|_| CryptoError::Rng)?;
    let tmp = dir.join(format!(".hermes-tmp-{}", crate::hexutil::encode(&rnd)));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    #[cfg(unix)]
    {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}
