//! Platform-independent tracking engine: turns raw OS observations into a clean event timeline.
//! Everything OS-specific is behind [`Platform`], so every rule below is unit-tested on any host.

use crate::model::*;

/// Applications whose windows are never described (best effort; see docs/SECURITY.md).
/// Credential managers and OS authentication surfaces. Extendable via `Config::exclusions`.
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    "keepass.exe",
    "keepassxc.exe",
    "1password.exe",
    "bitwarden.exe",
    "lastpass.exe",
    "dashlane.exe",
    "enpass.exe",
    "logonui.exe",
    "consent.exe",
    "credentialuibroker.exe",
    "credentialenrollmentmanager.exe",
];

#[derive(Debug)]
pub enum DescribeError {
    /// The window no longer exists.
    WindowGone,
}

pub trait Platform {
    fn wall_us(&self) -> i64;
    fn mono_us(&self) -> u64;
    fn new_session_id(&self) -> u64;
    fn foreground(&self) -> Option<u64>;
    fn describe(&self, hwnd: u64) -> Result<Focus, DescribeError>;
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Lower-case executable names to exclude (in addition to nothing: pass DEFAULT_EXCLUSIONS to keep them).
    pub exclusions: Vec<String>,
    /// A title change is reported after the title has been quiet for this long...
    pub title_coalesce_us: u64,
    /// ...and never more often than this per collector (rate limit for apps that animate their title).
    pub title_min_interval_us: u64,
    pub max_title_chars: usize,
    pub version: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            exclusions: DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect(),
            title_coalesce_us: 500_000,
            title_min_interval_us: 1_000_000,
            max_title_chars: 2048,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// One event ready for the encrypted store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emit {
    pub ts_us: i64,
    pub session_id: u64,
    pub kind: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// Foreground window changed (None = no foreground window).
    Foreground(Option<u64>),
    /// Something suggests this window's title changed (cheap hint; re-read after coalescing).
    TitleHint(u64),
    Destroyed(u64),
    Lock,
    Unlock,
    Suspend,
    Resume,
    Logoff,
    Disconnect,
    Reconnect,
    Pause,
    ResumeCollection,
    Stop,
}

/// What the collector is doing right now (drives the tray indicator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionState {
    /// A session is open: events are being recorded.
    Recording,
    /// Paused by the user (nothing is read or recorded).
    Paused,
    /// Not recording because the session is locked / logged off.
    Locked,
    /// Not recording because the machine is asleep.
    Asleep,
    /// Not recording because the (remote) session is disconnected.
    Disconnected,
    Stopped,
}

/// Observations that end or suspend recording must be made durable immediately: the machine may
/// sleep or power off before any timer fires.
pub fn needs_durable_flush(o: &Observation) -> bool {
    matches!(o, Observation::Lock | Observation::Suspend | Observation::Logoff | Observation::Disconnect | Observation::Pause | Observation::Stop)
}

const F_PAUSED: u8 = 1;
const F_LOCKED: u8 = 2;
const F_ASLEEP: u8 = 4;
const F_DISCONNECTED: u8 = 8;
const F_STOPPED: u8 = 16;

struct Current {
    focus: Focus,
    since_mono: u64,
}

pub struct Engine<P: Platform> {
    p: P,
    cfg: Config,
    flags: u8,
    session: Option<u64>,
    cur: Option<Current>,
    last_lost_hwnd: Option<u64>,
    /// Window we know was destroyed: the OS may keep reporting it as foreground for a moment.
    dead_hwnd: Option<u64>,
    pending_title_due: Option<u64>,
    last_title_emit_mono: Option<u64>,
}

impl<P: Platform> Engine<P> {
    pub fn new(p: P, mut cfg: Config) -> Self {
        for e in cfg.exclusions.iter_mut() {
            *e = e.to_lowercase();
        }
        Engine {
            p,
            cfg,
            flags: 0,
            session: None,
            cur: None,
            last_lost_hwnd: None,
            dead_hwnd: None,
            pending_title_due: None,
            last_title_emit_mono: None,
        }
    }

    pub fn platform(&self) -> &P {
        &self.p
    }
    pub fn session_id(&self) -> Option<u64> {
        self.session
    }
    pub fn is_active(&self) -> bool {
        self.flags == 0
    }
    pub fn is_stopped(&self) -> bool {
        self.flags & F_STOPPED != 0
    }

    pub fn state(&self) -> CollectionState {
        if self.flags & F_STOPPED != 0 {
            CollectionState::Stopped
        } else if self.flags & F_PAUSED != 0 {
            CollectionState::Paused
        } else if self.flags & F_LOCKED != 0 {
            CollectionState::Locked
        } else if self.flags & F_ASLEEP != 0 {
            CollectionState::Asleep
        } else if self.flags & F_DISCONNECTED != 0 {
            CollectionState::Disconnected
        } else {
            CollectionState::Recording
        }
    }

    /// Start in the paused state: no session is opened and nothing is recorded until `ResumeCollection`.
    pub fn start_paused(&mut self) {
        self.flags |= F_PAUSED;
    }

    /// Begin collection: opens the first session and reports the current foreground window.
    pub fn start(&mut self, out: &mut Vec<Emit>) {
        self.begin_session(StartReason::CollectorStart, out);
    }

    /// Monotonic time at which `tick` should next be called (None = no timer needed).
    pub fn next_deadline(&self) -> Option<u64> {
        self.pending_title_due
    }

    pub fn tick(&mut self, out: &mut Vec<Emit>) {
        let now = self.p.mono_us();
        if let Some(due) = self.pending_title_due {
            if now >= due {
                self.pending_title_due = None;
                self.refresh_title(out);
            }
        }
    }

    pub fn handle(&mut self, obs: Observation, out: &mut Vec<Emit>) {
        if self.is_stopped() {
            return;
        }
        match obs {
            Observation::Foreground(h) => {
                if self.is_active() {
                    self.on_foreground(h, out);
                }
            }
            Observation::TitleHint(h) => {
                if self.is_active()
                    && self.cur.as_ref().map(|c| c.focus.window.hwnd) == Some(h)
                    && self.pending_title_due.is_none()
                {
                    let now = self.p.mono_us();
                    let mut due = now + self.cfg.title_coalesce_us;
                    if let Some(last) = self.last_title_emit_mono {
                        due = due.max(last + self.cfg.title_min_interval_us);
                    }
                    self.pending_title_due = Some(due);
                }
            }
            Observation::Destroyed(h) => {
                if self.is_active() && self.cur.as_ref().map(|c| c.focus.window.hwnd) == Some(h) {
                    self.lose(LostReason::WindowClosed, out);
                    self.dead_hwnd = Some(h);
                    self.sync_foreground(out);
                }
            }
            Observation::Lock => self.set_flag(F_LOCKED, EndReason::Lock, out),
            Observation::Suspend => self.set_flag(F_ASLEEP, EndReason::Sleep, out),
            Observation::Logoff => self.set_flag(F_LOCKED, EndReason::Logoff, out),
            Observation::Disconnect => self.set_flag(F_DISCONNECTED, EndReason::Disconnect, out),
            Observation::Pause => self.set_flag(F_PAUSED, EndReason::Pause, out),
            Observation::Stop => {
                self.end_session(EndReason::CollectorStop, out);
                self.flags |= F_STOPPED;
            }
            Observation::Unlock => self.clear_flag(F_LOCKED, StartReason::Unlock, out),
            Observation::Resume => self.clear_flag(F_ASLEEP, StartReason::Resume, out),
            Observation::Reconnect => self.clear_flag(F_DISCONNECTED, StartReason::Reconnect, out),
            Observation::ResumeCollection => {
                self.clear_flag(F_PAUSED, StartReason::ResumeCollection, out)
            }
        }
    }

    // ------------------------------------------------------------------ sessions

    fn set_flag(&mut self, flag: u8, reason: EndReason, out: &mut Vec<Emit>) {
        let was_active = self.is_active();
        self.flags |= flag;
        if was_active {
            self.end_session(reason, out);
        }
    }

    fn clear_flag(&mut self, flag: u8, reason: StartReason, out: &mut Vec<Emit>) {
        if self.flags & flag == 0 {
            return;
        }
        self.flags &= !flag;
        if self.is_active() {
            self.begin_session(reason, out);
        }
    }

    fn begin_session(&mut self, reason: StartReason, out: &mut Vec<Emit>) {
        let id = self.p.new_session_id();
        self.session = Some(id);
        self.cur = None;
        self.pending_title_due = None;
        self.last_lost_hwnd = None;
        self.emit(
            ActivityEvent::SessionStart {
                reason,
                version: self.cfg.version.clone(),
            },
            out,
        );
        self.sync_foreground(out);
    }

    fn end_session(&mut self, reason: EndReason, out: &mut Vec<Emit>) {
        if self.session.is_none() {
            return;
        }
        self.lose(LostReason::SessionEnd, out);
        self.emit(ActivityEvent::SessionEnd { reason }, out);
        self.session = None;
        self.pending_title_due = None;
    }

    // ------------------------------------------------------------------ focus

    fn sync_foreground(&mut self, out: &mut Vec<Emit>) {
        let fg = self.p.foreground();
        self.on_foreground(fg, out);
    }

    fn on_foreground(&mut self, fg: Option<u64>, out: &mut Vec<Emit>) {
        let Some(h) = fg else {
            if self.cur.is_some() {
                self.lose(LostReason::NoForegroundWindow, out);
            }
            return;
        };
        if self.dead_hwnd == Some(h) {
            // Still reported as foreground although we saw it destroyed: ignore until the OS moves on.
            return;
        }
        self.dead_hwnd = None;
        match self.p.describe(h) {
            Err(DescribeError::WindowGone) => {
                if self.cur.is_some() {
                    self.lose(LostReason::WindowClosed, out);
                }
            }
            Ok(raw) => {
                let f = self.sanitize(raw);
                if let Some(c) = &self.cur {
                    if same_window(&c.focus, &f) {
                        if c.focus.window.title != f.window.title
                            || c.focus.window.title_status != f.window.title_status
                        {
                            // Same window, different title: report through the coalescing path.
                            self.handle(Observation::TitleHint(h), out);
                        }
                        return;
                    }
                    self.lose(LostReason::Switched, out);
                }
                self.gain(f, out);
            }
        }
    }

    fn gain(&mut self, f: Focus, out: &mut Vec<Emit>) {
        let ev = ActivityEvent::FocusGained {
            app: f.app.clone(),
            window: f.window.clone(),
            from_hwnd: self.last_lost_hwnd,
        };
        self.cur = Some(Current {
            focus: f,
            since_mono: self.p.mono_us(),
        });
        self.pending_title_due = None;
        self.emit(ev, out);
    }

    fn lose(&mut self, reason: LostReason, out: &mut Vec<Emit>) {
        if let Some(c) = self.cur.take() {
            let focus_us = self.p.mono_us().saturating_sub(c.since_mono);
            self.last_lost_hwnd = Some(c.focus.window.hwnd);
            self.pending_title_due = None;
            self.emit(
                ActivityEvent::FocusLost {
                    reason,
                    hwnd: c.focus.window.hwnd,
                    pid: c.focus.app.pid,
                    focus_us,
                },
                out,
            );
        }
    }

    fn refresh_title(&mut self, out: &mut Vec<Emit>) {
        let Some(h) = self.cur.as_ref().map(|c| c.focus.window.hwnd) else {
            return;
        };
        match self.p.describe(h) {
            Err(DescribeError::WindowGone) => {
                self.lose(LostReason::WindowClosed, out);
                self.sync_foreground(out);
            }
            Ok(raw) => {
                let f = self.sanitize(raw);
                let c = self.cur.as_mut().expect("checked above");
                if !same_window(&c.focus, &f) {
                    // The hwnd now belongs to something else: treat as a switch.
                    self.sync_foreground(out);
                    return;
                }
                if c.focus.window.title == f.window.title
                    && c.focus.window.title_status == f.window.title_status
                {
                    return; // coalesced away: nothing actually changed
                }
                c.focus.window.title = f.window.title.clone();
                c.focus.window.title_status = f.window.title_status;
                c.focus.window.title_truncated = f.window.title_truncated;
                let ev = ActivityEvent::TitleChanged {
                    hwnd: h,
                    title: f.window.title,
                    title_status: f.window.title_status,
                    title_truncated: f.window.title_truncated,
                };
                self.last_title_emit_mono = Some(self.p.mono_us());
                self.emit(ev, out);
            }
        }
    }

    /// Applies exclusions and title limits. Excluded applications are recorded only as
    /// "an excluded application had focus": no path, no name, no class, no title.
    fn sanitize(&self, mut f: Focus) -> Focus {
        let name = f.app.exe_name.as_deref().map(|n| n.to_lowercase());
        let host = f.app.host_exe.as_deref().map(|n| n.to_lowercase());
        let excluded = [name, host]
            .into_iter()
            .flatten()
            .any(|n| self.cfg.exclusions.iter().any(|e| *e == n));
        if excluded {
            f.app.exe_name = None;
            f.app.exe_path = None;
            f.app.host_exe = None;
            f.app.status = IdentityStatus::Excluded;
            f.window.class = None;
            f.window.title = None;
            f.window.title_status = TitleStatus::Redacted;
            f.window.title_truncated = false;
        } else if let Some(t) = &f.window.title {
            if t.chars().count() > self.cfg.max_title_chars {
                f.window.title = Some(t.chars().take(self.cfg.max_title_chars).collect());
                f.window.title_truncated = true;
            }
        }
        f
    }

    fn emit(&mut self, ev: ActivityEvent, out: &mut Vec<Emit>) {
        let Some(session_id) = self.session else {
            return;
        };
        out.push(Emit {
            ts_us: self.p.wall_us(),
            session_id,
            kind: ev.kind(),
            body: encode_body(self.p.mono_us(), &ev),
        });
    }
}

fn same_window(a: &Focus, b: &Focus) -> bool {
    a.window.hwnd == b.window.hwnd
        && a.app.pid == b.app.pid
        && a.app.process_start == b.app.process_start
}
