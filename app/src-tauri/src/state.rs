use crate::prefs::Prefs;
use crate::segments::Seg;
use hermes_viewer::Archive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// The unlocked archive. Exists only in this process's memory; dropping it is "locking".
pub struct Session {
    pub archive: Archive,
    pub segs: Vec<Seg>,
    /// The Share B key files used to unlock (paths only), so the integrity check can be re-run.
    pub key_files: Vec<PathBuf>,
    pub opened_us: i64,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub session: RwLock<Option<Arc<Session>>>,
    pub prefs: Mutex<Prefs>,
    pub stats_cache: Mutex<Option<(Instant, crate::commands::ArchiveStats)>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, prefs: Prefs) -> Self {
        AppState { data_dir, session: RwLock::new(None), prefs: Mutex::new(prefs), stats_cache: Mutex::new(None) }
    }

    pub fn prefs_path(&self) -> PathBuf {
        prefs_path(&self.data_dir)
    }

    pub fn offset(&self) -> i32 {
        let p = self.prefs.lock().unwrap();
        p.tz_override_minutes.unwrap_or_else(hermes_cli::usb::local_offset_minutes)
    }

    pub fn session(&self) -> Result<Arc<Session>, String> {
        self.session.read().unwrap().clone().ok_or_else(|| "The archive is locked. Unlock it first.".to_string())
    }

    pub fn rules(&self) -> Vec<crate::prefs::Rule> {
        self.prefs.lock().unwrap().rules.clone()
    }
}

pub fn prefs_path(data_dir: &Path) -> PathBuf {
    data_dir.join("hermes-app.json")
}

pub fn now_us() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0)
}

/// `--data-dir DIR`, else `%LOCALAPPDATA%\Hermes` (the same default as the `hermes` command).
pub fn resolve_data_dir() -> PathBuf {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--data-dir" {
            if let Some(v) = it.next() {
                return PathBuf::from(v);
            }
        }
    }
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Hermes")).unwrap_or_else(|| PathBuf::from("Hermes"))
}
