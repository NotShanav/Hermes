//! Live collector state for other Hermes programs (the desktop app).
//!
//! The tray owns the collection state (recording / paused / locked / ...), and a second process cannot ask
//! for it. The tray therefore writes `<data-dir>/collector-state.json` whenever the state changes and
//! whenever something becomes durable. The file holds counters and a state name only: never archive
//! content. It is advisory: readers must also check that a collector is actually running.

use hermes_collector::{warnings, Status};
use hermes_tracker::CollectionState;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct LiveStatus {
    /// recording | paused | locked | asleep | disconnected | stopped
    pub state: String,
    pub pid: u32,
    pub events_recorded: u64,
    pub events_dropped: u64,
    pub store_errors: u64,
    pub last_error: Option<String>,
    pub last_flush_us: i64,
    pub warnings: Vec<String>,
    pub updated_us: i64,
}

pub fn state_key(s: CollectionState) -> &'static str {
    match s {
        CollectionState::Recording => "recording",
        CollectionState::Paused => "paused",
        CollectionState::Locked => "locked",
        CollectionState::Asleep => "asleep",
        CollectionState::Disconnected => "disconnected",
        CollectionState::Stopped => "stopped",
    }
}

pub fn live_path(data_dir: &Path) -> PathBuf {
    data_dir.join("collector-state.json")
}

pub fn snapshot(state: CollectionState, status: &Status, now_us: i64) -> LiveStatus {
    LiveStatus {
        state: state_key(state).to_string(),
        pid: std::process::id(),
        events_recorded: status.events_recorded,
        events_dropped: status.events_dropped,
        store_errors: status.store_errors,
        last_error: status.last_error.clone(),
        last_flush_us: status.last_flush_us,
        warnings: warnings(status),
        updated_us: now_us,
    }
}

/// Best-effort atomic write (temp file + rename). Failure never affects recording.
pub fn write(path: &Path, live: &LiveStatus) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(serde_json::to_string(live).expect("live status serialises").as_bytes())?;
    }
    std::fs::rename(&tmp, path)
}

pub fn read(path: &Path) -> Option<LiveStatus> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_state_names() {
        let d = tempfile::tempdir().unwrap();
        let p = live_path(d.path());
        let st = Status { events_recorded: 7, events_dropped: 1, ..Status::default() };
        let s = snapshot(CollectionState::Paused, &st, 42);
        write(&p, &s).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.state, "paused");
        assert_eq!(back.events_recorded, 7);
        assert!(back.warnings.iter().any(|w| w.contains("could not be stored")));
        assert_eq!(read(&d.path().join("missing.json")), None);
        assert_eq!(state_key(CollectionState::Recording), "recording");
    }
}
