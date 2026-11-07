//! Collector/security status shown by the tray and printed by the console build.

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub data_dir: PathBuf,
    pub key_id: String,
    pub events_recorded: u64,
    pub events_dropped: u64,
    pub store_errors: u64,
    pub last_error: Option<String>,
    /// Wall-clock µs of the last successful flush (0 = none yet this run).
    pub last_flush_us: i64,
    pub unclean_previous_shutdown: bool,
    pub integrity_check_ran: bool,
    pub repaired: Option<String>,
    pub exclusions: usize,
}

/// Conditions the user should know about; non-empty = the tray shows a warning indicator.
pub fn warnings(s: &Status) -> Vec<String> {
    let mut w = vec![];
    if let Some(e) = &s.last_error {
        w.push(format!("storage problem: {e}"));
    }
    if s.events_dropped > 0 {
        w.push(format!("{} event(s) could not be stored this run", s.events_dropped));
    }
    if let Some(r) = &s.repaired {
        w.push(format!("database was damaged and repaired ({r})"));
    }
    if s.unclean_previous_shutdown && s.repaired.is_none() {
        w.push("the previous run did not shut down cleanly (a marker was written; at most the last ~2 s were lost)".into());
    }
    w
}

fn hhmmss(us: i64) -> String {
    let secs = us.div_euclid(1_000_000).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02} UTC", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Human-readable status block. `state` is the collection state label supplied by the UI.
pub fn format_status(s: &Status, state: &str, autostart: bool) -> String {
    let mut o = String::new();
    o.push_str(&format!("Collection: {state}\n"));
    o.push_str(&format!("Events recorded this run: {}   dropped: {}\n", s.events_recorded, s.events_dropped));
    o.push_str(&format!("Last saved to disk: {}\n", if s.last_flush_us == 0 { "nothing saved yet".to_string() } else { hhmmss(s.last_flush_us) }));
    o.push_str("\nSecurity\n");
    o.push_str(&format!("  Encrypted with key set {}\n", short(&s.key_id)));
    o.push_str("  Records are encrypted before they reach the disk; checkpoints are signed\n");
    o.push_str("  This program holds only the PUBLIC key: it cannot read what it recorded\n");
    o.push_str("  Reading the history needs BOTH key shares (this PC + your USB drive)\n");
    o.push_str("  The USB drive is not needed for recording\n");
    o.push_str(&format!("  Applications excluded from recording: {}\n", s.exclusions));
    o.push_str(&format!("  Start with Windows: {}\n", if autostart { "on" } else { "off" }));
    o.push_str(&format!("\nArchive: {}\n", s.data_dir.display()));
    o.push_str(&format!("Integrity check at start: {}\n", if s.integrity_check_ran { "ran" } else { "skipped (clean previous shutdown, checked within 24 h)" }));
    let w = warnings(s);
    if w.is_empty() {
        o.push_str("\nNo problems detected.\n");
    } else {
        o.push_str("\nATTENTION\n");
        for x in w {
            o.push_str(&format!("  ! {x}\n"));
        }
    }
    o
}

fn short(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}
