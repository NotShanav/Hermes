//! Activity event model. Bodies are JSON (versioned, self-describing) and are stored INSIDE the
//! encrypted record envelope; the numeric `kind` below is also inside the envelope.

use serde::{Deserialize, Serialize};

pub mod kinds {
    pub const SESSION_START: u16 = 0x0100;
    pub const SESSION_END: u16 = 0x0101;
    pub const FOCUS_GAINED: u16 = 0x0110;
    pub const FOCUS_LOST: u16 = 0x0111;
    pub const TITLE_CHANGED: u16 = 0x0112;
}

pub const BODY_VERSION: u8 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStatus {
    /// Executable path and name were read.
    Resolved,
    /// Application is on the exclusion list: nothing identifying is recorded.
    Excluded,
    /// The OS refused access to the process (protected/elevated/system process).
    AccessDenied,
    /// Process had already exited when queried.
    ProcessGone,
    /// Any other failure.
    Unavailable,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TitleStatus {
    Text,
    Empty,
    Unavailable,
    /// Withheld because the application is excluded.
    Redacted,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    pub pid: u32,
    /// Process creation time (Windows FILETIME). Together with `pid` it identifies one process
    /// instance, so a reused PID is never mistaken for the previous process.
    pub process_start: Option<u64>,
    pub exe_path: Option<String>,
    pub exe_name: Option<String>,
    pub status: IdentityStatus,
    /// Set for packaged (UWP) apps whose window is hosted by another process (e.g. ApplicationFrameHost.exe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_exe: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub hwnd: u64,
    pub class: Option<String>,
    pub title: Option<String>,
    pub title_status: TitleStatus,
    #[serde(default, skip_serializing_if = "is_false")]
    pub title_truncated: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Snapshot of the foreground window + its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub app: AppIdentity,
    pub window: WindowInfo,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartReason {
    CollectorStart,
    Unlock,
    Resume,
    ResumeCollection,
    Reconnect,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    CollectorStop,
    Lock,
    Sleep,
    Pause,
    Logoff,
    Disconnect,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LostReason {
    /// Another window took the foreground.
    Switched,
    /// No foreground window (desktop transition, secure desktop such as UAC / Ctrl+Alt+Del, or query failed).
    NoForegroundWindow,
    /// The window was destroyed.
    WindowClosed,
    /// Collection session ended (lock, sleep, pause, stop...).
    SessionEnd,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ActivityEvent {
    SessionStart {
        reason: StartReason,
        version: String,
    },
    SessionEnd {
        reason: EndReason,
    },
    FocusGained {
        app: AppIdentity,
        window: WindowInfo,
        /// Window that had focus immediately before, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_hwnd: Option<u64>,
    },
    FocusLost {
        reason: LostReason,
        hwnd: u64,
        pid: u32,
        /// How long the window held focus, measured on the monotonic clock (immune to clock changes).
        focus_us: u64,
    },
    TitleChanged {
        hwnd: u64,
        title: Option<String>,
        title_status: TitleStatus,
        #[serde(default, skip_serializing_if = "is_false")]
        title_truncated: bool,
    },
}

impl ActivityEvent {
    pub fn kind(&self) -> u16 {
        match self {
            ActivityEvent::SessionStart { .. } => kinds::SESSION_START,
            ActivityEvent::SessionEnd { .. } => kinds::SESSION_END,
            ActivityEvent::FocusGained { .. } => kinds::FOCUS_GAINED,
            ActivityEvent::FocusLost { .. } => kinds::FOCUS_LOST,
            ActivityEvent::TitleChanged { .. } => kinds::TITLE_CHANGED,
        }
    }
}

/// What is actually stored (JSON) as the record body.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EventBody {
    pub v: u8,
    /// Monotonic microseconds since the collector process started (ordering/duration aid).
    pub mono_us: u64,
    #[serde(flatten)]
    pub event: ActivityEvent,
}

pub fn encode_body(mono_us: u64, event: &ActivityEvent) -> Vec<u8> {
    serde_json::to_vec(&EventBody {
        v: BODY_VERSION,
        mono_us,
        event: event.clone(),
    })
    .expect("event serialisation cannot fail")
}

pub fn decode_body(bytes: &[u8]) -> Result<EventBody, serde_json::Error> {
    serde_json::from_slice(bytes)
}
