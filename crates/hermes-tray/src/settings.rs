//! User settings (`<data-dir>/settings.json`). Plain, human-editable, never contains archive content.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Begin recording as soon as Hermes starts (otherwise it starts paused).
    pub start_collecting_on_launch: bool,
    /// Small always-on-top red dot (in addition to the tray icon, which Windows may hide in the overflow area).
    pub show_indicator_dot: bool,
    /// Extra executable names that are never described (see docs/MILESTONE1.md).
    pub exclusions: Vec<String>,
    /// Keep the built-in credential-manager exclusions.
    pub default_exclusions: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { start_collecting_on_launch: true, show_indicator_dot: true, exclusions: vec![], default_exclusions: true }
    }
}

impl Settings {
    /// Missing file = defaults. A damaged file is NOT overwritten: defaults are used and a warning is returned.
    pub fn load(path: &Path) -> (Settings, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => (s, None),
                Err(e) => (Settings::default(), Some(format!("{} is not valid ({e}); using defaults and leaving the file untouched", path.display()))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(format!("cannot read {}: {e}; using defaults", path.display()))),
        }
    }

    /// Crash-safe write (temp file + fsync + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(serde_json::to_string_pretty(self).expect("settings serialise").as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    }

    /// Complete exclusion list handed to the tracker.
    pub fn effective_exclusions(&self) -> Vec<String> {
        let mut v: Vec<String> = if self.default_exclusions { hermes_tracker::DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect() } else { vec![] };
        v.extend(self.exclusions.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        v
    }
}
