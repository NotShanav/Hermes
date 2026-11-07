//! The tray menu as data. The Windows layer only materialises it, so every label, enabled flag and
//! check mark is unit-tested without a desktop.

use hermes_tracker::CollectionState;

pub mod ids {
    pub const PAUSE_RESUME: u32 = 1001;
    pub const PAUSE_15: u32 = 1002;
    pub const PAUSE_60: u32 = 1003;
    pub const STATUS: u32 = 1004;
    pub const OPEN_VIEWER: u32 = 1005;
    pub const AUTOSTART: u32 = 1006;
    pub const START_ON_LAUNCH: u32 = 1007;
    pub const DOT: u32 = 1008;
    pub const OPEN_FOLDER: u32 = 1009;
    pub const OPEN_SETTINGS: u32 = 1010;
    pub const EXIT: u32 = 1099;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: u32, // 0 for separators and the informational lines
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    pub separator: bool,
}

impl Item {
    fn action(id: u32, label: &str) -> Item {
        Item { id, label: label.into(), enabled: true, checked: false, separator: false }
    }
    fn info(label: String) -> Item {
        Item { id: 0, label, enabled: false, checked: false, separator: false }
    }
    fn sep() -> Item {
        Item { id: 0, label: String::new(), enabled: false, checked: false, separator: true }
    }
    fn toggle(id: u32, label: &str, on: bool) -> Item {
        Item { checked: on, ..Item::action(id, label) }
    }
}

pub struct MenuInput {
    pub state: CollectionState,
    /// e.g. "1,234 events this run · saved 14:02:11 UTC"
    pub summary: String,
    pub warnings: usize,
    pub autostart: bool,
    pub start_on_launch: bool,
    pub dot: bool,
    pub viewer_available: bool,
    /// e.g. "auto-resumes at 15:20"
    pub pause_note: Option<String>,
}

pub fn state_label(s: CollectionState) -> &'static str {
    match s {
        CollectionState::Recording => "Recording",
        CollectionState::Paused => "Paused (not recording)",
        CollectionState::Locked => "Not recording (screen locked)",
        CollectionState::Asleep => "Not recording (computer asleep)",
        CollectionState::Disconnected => "Not recording (session disconnected)",
        CollectionState::Stopped => "Stopped",
    }
}

pub fn tooltip(s: CollectionState, summary: &str) -> String {
    let t = format!("Hermes: {}\n{}", state_label(s), summary);
    t.chars().take(127).collect() // NOTIFYICONDATA tip limit: 127 chars + NUL
}

pub fn build(i: &MenuInput) -> Vec<Item> {
    let mut m = Vec::new();
    let head = match i.state {
        CollectionState::Recording => "● Hermes is RECORDING".to_string(),
        CollectionState::Paused => format!("Ⅱ Hermes is PAUSED{}", i.pause_note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default()),
        other => format!("○ Hermes: {}", state_label(other)),
    };
    m.push(Item::info(head));
    m.push(Item::info(i.summary.clone()));
    if i.warnings > 0 {
        m.push(Item::info(format!("⚠ {} warning(s): open Status for details", i.warnings)));
    }
    m.push(Item::sep());
    let can_pause = i.state == CollectionState::Recording;
    let can_resume = i.state == CollectionState::Paused;
    m.push(Item { enabled: can_pause || can_resume, ..Item::action(ids::PAUSE_RESUME, if can_resume { "Resume collection" } else { "Pause collection" }) });
    m.push(Item { enabled: can_pause, ..Item::action(ids::PAUSE_15, "Pause for 15 minutes") });
    m.push(Item { enabled: can_pause, ..Item::action(ids::PAUSE_60, "Pause for 1 hour") });
    m.push(Item::sep());
    m.push(Item::action(ids::STATUS, "Status and security…"));
    if i.viewer_available {
        m.push(Item::action(ids::OPEN_VIEWER, "Open Hermes Viewer"));
    } else {
        m.push(Item { enabled: false, ..Item::action(ids::OPEN_VIEWER, "Open Hermes Viewer (not installed yet)") });
    }
    m.push(Item::sep());
    m.push(Item::toggle(ids::AUTOSTART, "Start with Windows", i.autostart));
    m.push(Item::toggle(ids::START_ON_LAUNCH, "Start recording when Hermes starts", i.start_on_launch));
    m.push(Item::toggle(ids::DOT, "Show on-screen recording dot", i.dot));
    m.push(Item::action(ids::OPEN_FOLDER, "Open data folder"));
    m.push(Item::action(ids::OPEN_SETTINGS, "Edit settings and exclusions…"));
    m.push(Item::sep());
    m.push(Item::action(ids::EXIT, "Exit Hermes (stops recording)"));
    m
}

/// Command line registered for "Start with Windows". The tray starts visibly; there is no hidden mode.
pub fn autostart_command(exe: &str, data_dir: &str) -> String {
    format!("\"{exe}\" --data-dir \"{data_dir}\"")
}

/// One-line summary used in the menu and tooltip.
pub fn summary(recorded: u64, dropped: u64, last_flush_us: i64) -> String {
    let saved = if last_flush_us == 0 {
        "nothing saved yet".to_string()
    } else {
        let s = last_flush_us.div_euclid(1_000_000).rem_euclid(86_400);
        format!("saved {:02}:{:02}:{:02} UTC", s / 3600, s % 3600 / 60, s % 60)
    };
    if dropped > 0 {
        format!("{recorded} events ({dropped} dropped) · {saved}")
    } else {
        format!("{recorded} events · {saved}")
    }
}
