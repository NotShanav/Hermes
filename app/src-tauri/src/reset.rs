//! "Clean slate": retire the current archive and keys and start a fresh archive with a new key set.
//!
//! Hermes's `init` never overwrites an existing archive or key file, and neither does this. The old files are
//! MOVED to `<data-dir>\reset-<timestamp>\` first, the normal `init` then creates a new archive and key set, and if
//! anything goes wrong everything is moved back. Only after the new archive exists are the old files deleted, and
//! only when the user explicitly asked for that.

use std::path::{Path, PathBuf};

/// Everything that makes up the old archive and its laptop-side key material.
const ENTRIES: &[&str] = &["hermes.db", "hermes.db-wal", "hermes.db-shm", "keys"];

pub struct MovedAside {
    pub dir: PathBuf,
    pub moved: Vec<String>,
}

pub fn move_aside(data_dir: &Path, stamp: i64) -> Result<MovedAside, String> {
    let dir = data_dir.join(format!("reset-{stamp}"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let mut done = MovedAside { dir: dir.clone(), moved: Vec::new() };
    for name in ENTRIES {
        let from = data_dir.join(name);
        if !from.exists() {
            continue;
        }
        if let Err(e) = std::fs::rename(&from, dir.join(name)) {
            restore(data_dir, &done);
            return Err(format!("could not move {name} aside: {e}\nClose anything that is using the Hermes data folder and try again. Nothing was changed."));
        }
        done.moved.push((*name).to_string());
    }
    Ok(done)
}

/// Puts the moved files back, first removing anything a failed `init` created in their place.
pub fn restore(data_dir: &Path, m: &MovedAside) {
    for name in &m.moved {
        let target = data_dir.join(name);
        if target.is_dir() {
            let _ = std::fs::remove_dir_all(&target);
        } else if target.exists() {
            let _ = std::fs::remove_file(&target);
        }
        let _ = std::fs::rename(m.dir.join(name), &target);
    }
    let _ = std::fs::remove_dir(&m.dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hermes-reset-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn moves_aside_and_restores_even_after_a_half_created_new_archive() {
        let d = scratch("a");
        std::fs::write(d.join("hermes.db"), b"OLD").unwrap();
        std::fs::create_dir_all(d.join("keys")).unwrap();
        std::fs::write(d.join("keys").join("a.sharea"), b"OLDKEY").unwrap();
        std::fs::write(d.join("settings.json"), b"{}").unwrap();

        let m = move_aside(&d, 1).unwrap();
        assert!(!d.join("hermes.db").exists() && !d.join("keys").exists());
        assert!(d.join("settings.json").exists(), "settings are not part of the archive");
        assert_eq!(std::fs::read(m.dir.join("hermes.db")).unwrap(), b"OLD");

        // A failed init leaves a partial new archive behind.
        std::fs::write(d.join("hermes.db"), b"NEW").unwrap();
        std::fs::create_dir_all(d.join("keys")).unwrap();
        std::fs::write(d.join("keys").join("new.sharea"), b"NEWKEY").unwrap();

        restore(&d, &m);
        assert_eq!(std::fs::read(d.join("hermes.db")).unwrap(), b"OLD");
        assert_eq!(std::fs::read(d.join("keys").join("a.sharea")).unwrap(), b"OLDKEY");
        assert!(!d.join("keys").join("new.sharea").exists());
        assert!(!d.join("reset-1").exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
