use hermes_crypto::provision::load_viewer_material;
use hermes_crypto::shares::peek_share_b_key_id;
use hermes_crypto::{Opener, SecretProtector};
use hermes_db::{kind as sys, ArchiveReader, EventQuery, IntegrityReport, Keyring, OpenEventError};
use hermes_tracker::{decode_body, kinds, ActivityEvent, IdentityStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewKind {
    SessionStart,
    SessionEnd,
    FocusGained,
    FocusLost,
    TitleChanged,
    /// Storage-layer markers: dropped events, unclean shutdown/repair.
    System,
    /// Row written by the pre-HPKE format (unsigned, plaintext session/kind in the database).
    Legacy,
    /// Could not be opened: the key set it belongs to was not unlocked.
    Locked,
    /// Failed authentication: damaged or modified.
    Tampered,
    Unknown,
}

impl ViewKind {
    pub fn label(self) -> &'static str {
        match self {
            ViewKind::SessionStart => "SESSION START",
            ViewKind::SessionEnd => "SESSION END",
            ViewKind::FocusGained => "FOCUS",
            ViewKind::FocusLost => "FOCUS END",
            ViewKind::TitleChanged => "TITLE",
            ViewKind::System => "SYSTEM",
            ViewKind::Legacy => "LEGACY",
            ViewKind::Locked => "LOCKED",
            ViewKind::Tampered => "TAMPERED",
            ViewKind::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ViewEvent {
    pub seq: u64,
    pub ts_us: i64,
    pub session_id: u64,
    pub kind: ViewKind,
    /// Executable name, or a marker such as `<excluded application>` / `<access denied>`.
    pub app: String,
    pub exe_path: String,
    pub pid: u32,
    pub hwnd: u64,
    pub window_class: String,
    pub title: String,
    /// Reason / free text (session reasons, focus duration, system messages).
    pub detail: String,
    /// For FOCUS END rows: how long the window held focus (monotonic clock), in microseconds.
    pub focus_us: u64,
}

pub struct Archive {
    pub events: Vec<ViewEvent>,
    /// Human-readable notes about unlocking (which key sets are open/locked).
    pub notes: Vec<String>,
    pub registered_keys: usize,
    pub unlocked_keys: usize,
    pub locked: u64,
    pub tampered: u64,
    pub legacy_rows: u64,
    pub seq_gaps: Vec<(u64, u64)>,
    /// Number of trusted signature verifying keys obtained from the USB files.
    pub trust_anchors: usize,
    pub integrity: IntegrityReport,
}

fn app_name(a: &hermes_tracker::AppIdentity) -> String {
    match (&a.exe_name, a.status) {
        (Some(n), _) => n.clone(),
        (None, IdentityStatus::Excluded) => "<excluded application>".into(),
        (None, IdentityStatus::AccessDenied) => "<access denied>".into(),
        (None, IdentityStatus::ProcessGone) => "<process gone>".into(),
        _ => "<unknown>".into(),
    }
}

fn blank(seq: u64, ts_us: i64, session_id: u64, kind: ViewKind, detail: String) -> ViewEvent {
    ViewEvent { seq, ts_us, session_id, kind, app: String::new(), exe_path: String::new(), pid: 0, hwnd: 0, window_class: String::new(), title: String::new(), detail, focus_us: 0 }
}

impl Archive {
    /// Opens `<data_dir>/hermes.db` read-only and decrypts everything into memory.
    /// Needs, for every key set to be unlocked: Share A (this PC, via `protector`) AND its Share B file.
    pub fn open(data_dir: &Path, share_b_files: &[PathBuf], protector: &dyn SecretProtector) -> Result<Archive, String> {
        let db = data_dir.join("hermes.db");
        if !db.exists() {
            return Err(format!("no archive found at {} (run `hermes init` first)", db.display()));
        }
        let rd = ArchiveReader::open(&db).map_err(|e| e.to_string())?;
        let registered = rd.keys().map_err(|e| e.to_string())?;
        let keys_dir = data_dir.join("keys");
        let mut keyring = Keyring::new();
        let mut trust: HashMap<[u8; 16], [u8; 32]> = HashMap::new();
        let mut notes = vec![];
        let mut unlocked = 0usize;
        for p in share_b_files {
            let id = match peek_share_b_key_id(p) {
                Ok(id) => id,
                Err(e) => {
                    notes.push(format!("{}: not a usable Hermes key file ({e})", p.display()));
                    continue;
                }
            };
            match load_viewer_material(&keys_dir, &id, p, protector) {
                Ok((key, sign_pub)) => {
                    if let Some(sp) = sign_pub {
                        trust.insert(hermes_crypto::sign::signer_id(&sp), sp);
                    }
                    if keyring.has(&id) {
                        notes.push(format!("key set {id}: supplied twice"));
                    } else {
                        keyring.add(Opener::new(key));
                        unlocked += 1;
                        notes.push(format!("key set {id}: unlocked (Share A from this PC + Share B from {})", p.display()));
                    }
                }
                Err(e) => notes.push(format!("key set {id}: CANNOT unlock with {} ({e})", p.display())),
            }
        }
        if unlocked == 0 {
            return Err(format!("could not unlock any key set: both shares are required.\n  {}", notes.join("\n  ")));
        }
        for k in &registered {
            if !keyring.has(&k.info.key_id) {
                notes.push(format!("key set {} is LOCKED: its events will show as [locked] (supply its USB key file)", k.info.key_id));
            }
        }

        let mut events = Vec::new();
        let (mut locked, mut tampered, mut legacy_rows) = (0u64, 0u64, 0u64);
        let mut by_hwnd: HashMap<u64, (String, String, u32, String)> = HashMap::new();
        let mut after = None;
        loop {
            let page = rd.events(&EventQuery { after_seq: after, limit: Some(2000), ..Default::default() }).map_err(|e| e.to_string())?;
            if page.is_empty() {
                break;
            }
            for ev in &page {
                after = Some(ev.seq);
                let ve = match keyring.open_event(ev) {
                    Err(OpenEventError::MissingKey(id)) => {
                        locked += 1;
                        blank(ev.seq, ev.ts_us, 0, ViewKind::Locked, format!("key set {id} not unlocked"))
                    }
                    Err(OpenEventError::Tampered) => {
                        tampered += 1;
                        blank(ev.seq, ev.ts_us, 0, ViewKind::Tampered, "authentication failed: damaged or modified".into())
                    }
                    Err(OpenEventError::Other(e)) => blank(ev.seq, ev.ts_us, 0, ViewKind::Unknown, e.to_string()),
                    Ok(d) if d.legacy => {
                        legacy_rows += 1;
                        let text: String = String::from_utf8_lossy(&d.body).chars().take(120).collect();
                        blank(d.seq, d.ts_us, d.session_id, ViewKind::Legacy, format!("legacy record (kind {}): {text}", d.kind))
                    }
                    Ok(d) => match d.kind {
                        sys::SYSTEM_GAP | sys::SYSTEM_RECOVERED => blank(d.seq, d.ts_us, d.session_id, ViewKind::System, String::from_utf8_lossy(&d.body).into_owned()),
                        kinds::SESSION_START | kinds::SESSION_END | kinds::FOCUS_GAINED | kinds::FOCUS_LOST | kinds::TITLE_CHANGED => match decode_body(&d.body) {
                            Ok(b) => Self::from_activity(d.seq, d.ts_us, d.session_id, b.event, &mut by_hwnd),
                            Err(e) => blank(d.seq, d.ts_us, d.session_id, ViewKind::Unknown, format!("undecodable body: {e}")),
                        },
                        other => blank(d.seq, d.ts_us, d.session_id, ViewKind::Unknown, format!("unknown event kind {other:#06x} ({} bytes)", d.body.len())),
                    },
                };
                events.push(ve);
            }
        }
        let seq_gaps = rd.seq_gaps().map_err(|e| e.to_string())?;
        let integrity = rd.verify_integrity(&trust).map_err(|e| e.to_string())?;
        Ok(Archive { events, notes, registered_keys: registered.len(), unlocked_keys: unlocked, locked, tampered, legacy_rows, seq_gaps, trust_anchors: trust.len(), integrity })
    }

    fn from_activity(seq: u64, ts: i64, sid: u64, ev: ActivityEvent, by_hwnd: &mut HashMap<u64, (String, String, u32, String)>) -> ViewEvent {
        match ev {
            ActivityEvent::SessionStart { reason, version } => {
                by_hwnd.clear();
                blank(seq, ts, sid, ViewKind::SessionStart, format!("{reason:?} (collector {version})"))
            }
            ActivityEvent::SessionEnd { reason } => {
                by_hwnd.clear();
                blank(seq, ts, sid, ViewKind::SessionEnd, format!("{reason:?}"))
            }
            ActivityEvent::FocusGained { app, window, .. } => {
                let name = app_name(&app);
                let path = app.exe_path.clone().unwrap_or_default();
                let class = window.class.clone().unwrap_or_default();
                by_hwnd.insert(window.hwnd, (name.clone(), path.clone(), app.pid, class.clone()));
                let mut e = blank(seq, ts, sid, ViewKind::FocusGained, format!("{:?}{}", app.status, app.host_exe.as_deref().map(|h| format!(", hosted by {h}")).unwrap_or_default()));
                e.app = name;
                e.exe_path = path;
                e.pid = app.pid;
                e.hwnd = window.hwnd;
                e.window_class = class;
                e.title = window.title.unwrap_or_default();
                e
            }
            ActivityEvent::FocusLost { reason, hwnd, pid, focus_us } => {
                let known = by_hwnd.get(&hwnd).cloned();
                let mut e = blank(seq, ts, sid, ViewKind::FocusLost, format!("{reason:?}, in focus {:.1}s", focus_us as f64 / 1e6));
                e.hwnd = hwnd;
                e.pid = pid;
                if let Some((n, p, _, c)) = known {
                    e.app = n;
                    e.exe_path = p;
                    e.window_class = c;
                }
                e.focus_us = focus_us;
                e
            }
            ActivityEvent::TitleChanged { hwnd, title, .. } => {
                let mut e = blank(seq, ts, sid, ViewKind::TitleChanged, String::new());
                e.hwnd = hwnd;
                e.title = title.unwrap_or_default();
                if let Some((n, p, pid, c)) = by_hwnd.get(&hwnd) {
                    e.app = n.clone();
                    e.exe_path = p.clone();
                    e.pid = *pid;
                    e.window_class = c.clone();
                }
                e
            }
        }
    }

    pub fn select(&self, f: &Filter) -> Vec<&ViewEvent> {
        self.events.iter().filter(|e| f.matches(e)).collect()
    }

    /// Time in focus per application (sum of focus durations) for the events matching the filter's date range.
    pub fn summary(&self, f: &Filter) -> Vec<AppTime> {
        let mut m: HashMap<String, (u64, u64)> = HashMap::new();
        for e in self.events.iter().filter(|e| f.matches_time_session(e)) {
            match e.kind {
                ViewKind::FocusGained if !e.app.is_empty() => m.entry(e.app.clone()).or_default().1 += 1,
                ViewKind::FocusLost if !e.app.is_empty() => {
                    m.entry(e.app.clone()).or_default().0 += e.focus_us;
                }
                _ => {}
            }
        }
        let mut v: Vec<AppTime> = m.into_iter().map(|(app, (focus_us, switches))| AppTime { app, focus_us, switches }).collect();
        v.sort_by(|a, b| b.focus_us.cmp(&a.focus_us).then(a.app.cmp(&b.app)));
        v
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppTime {
    pub app: String,
    pub focus_us: u64,
    pub switches: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub from_us: Option<i64>,
    pub to_us: Option<i64>,
    /// Case-insensitive substring of the application name.
    pub app: Option<String>,
    /// Case-insensitive substring searched in application, path, window class, title and detail.
    pub text: Option<String>,
    pub session: Option<u64>,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        self.from_us.is_none() && self.to_us.is_none() && self.app.is_none() && self.text.is_none() && self.session.is_none()
    }

    fn matches_time_session(&self, e: &ViewEvent) -> bool {
        self.from_us.map_or(true, |f| e.ts_us >= f) && self.to_us.map_or(true, |t| e.ts_us < t) && self.session.map_or(true, |s| e.session_id == s)
    }

    pub fn matches(&self, e: &ViewEvent) -> bool {
        if !self.matches_time_session(e) {
            return false;
        }
        if let Some(a) = &self.app {
            if !e.app.to_lowercase().contains(&a.to_lowercase()) {
                return false;
            }
        }
        if let Some(t) = &self.text {
            let t = t.to_lowercase();
            let hay = [&e.app, &e.exe_path, &e.window_class, &e.title, &e.detail];
            if !hay.iter().any(|h| h.to_lowercase().contains(&t)) {
                return false;
            }
        }
        true
    }
}
