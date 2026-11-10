//! The tray menu as data. The Windows layer only materialises it, so every label, enabled flag and
//! check mark is unit-tested without a desktop.

use crate::icon::IconKind;
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
    /// The status line at the top of the menu: clicking it opens the same status dialog as STATUS.
    pub const STATUS_HEADER: u32 = 1011;
    pub const EXIT: u32 = 1099;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: u32, // 0 for separators, submenus and the informational lines
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    pub separator: bool,
    /// Non-empty makes this item a submenu.
    pub children: Vec<Item>,
    /// A small coloured status picture shown beside the label (same artwork as the tray icon).
    pub icon: Option<IconKind>,
}

impl Item {
    fn action(id: u32, label: &str) -> Item {
        Item { id, label: label.into(), enabled: true, checked: false, separator: false, children: Vec::new(), icon: None }
    }
    fn info(label: String) -> Item {
        Item { id: 0, label, enabled: false, checked: false, separator: false, children: Vec::new(), icon: None }
    }
    fn sep() -> Item {
        Item { id: 0, label: String::new(), enabled: false, checked: false, separator: true, children: Vec::new(), icon: None }
    }
    fn toggle(id: u32, label: &str, on: bool) -> Item {
        Item { checked: on, ..Item::action(id, label) }
    }
    fn submenu(label: &str, enabled: bool, children: Vec<Item>) -> Item {
        Item { enabled, children, ..Item::action(0, label) }
    }
}

/// Every item, submenu contents included, in display order.
pub fn flatten(items: &[Item]) -> Vec<&Item> {
    let mut out = Vec::new();
    for i in items {
        out.push(i);
        out.extend(flatten(&i.children));
    }
    out
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
    // Status header: a coloured dot, a plain label, and a one-line summary. Clicking the header opens the status dialog.
    let (head, kind) = match i.state {
        CollectionState::Recording => ("Recording".to_string(), if i.warnings > 0 { IconKind::Warning } else { IconKind::Recording }),
        CollectionState::Paused => (format!("Paused{}", i.pause_note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default()), IconKind::Paused),
        other => (state_label(other).to_string(), IconKind::Inactive),
    };
    m.push(Item { icon: Some(kind), ..Item::action(ids::STATUS_HEADER, &head) });
    m.push(Item::info(i.summary.clone()));
    if i.warnings > 0 {
        m.push(Item::info(format!("{} warning(s): click the status line for details", i.warnings)));
    }
    m.push(Item::sep());
    let can_pause = i.state == CollectionState::Recording;
    let can_resume = i.state == CollectionState::Paused;
    m.push(Item { enabled: can_pause || can_resume, ..Item::action(ids::PAUSE_RESUME, if can_resume { "Resume recording" } else { "Pause recording" }) });
    m.push(Item::submenu(
        "Pause for\u{2026}",
        can_pause,
        vec![Item { enabled: can_pause, ..Item::action(ids::PAUSE_15, "15 minutes") }, Item { enabled: can_pause, ..Item::action(ids::PAUSE_60, "1 hour") }],
    ));
    m.push(Item::sep());
    if i.viewer_available {
        m.push(Item::action(ids::OPEN_VIEWER, "Open Hermes"));
    } else {
        m.push(Item { enabled: false, ..Item::action(ids::OPEN_VIEWER, "Open Hermes (not installed yet)") });
    }
    m.push(Item::action(ids::STATUS, "Status and security\u{2026}"));
    m.push(Item::action(ids::OPEN_FOLDER, "Open data folder"));
    m.push(Item::sep());
    m.push(Item::submenu(
        "Settings",
        true,
        vec![
            Item::toggle(ids::AUTOSTART, "Start with Windows", i.autostart),
            Item::toggle(ids::START_ON_LAUNCH, "Start recording when Hermes starts", i.start_on_launch),
            Item::toggle(ids::DOT, "Show on-screen recording dot", i.dot),
            Item::sep(),
            Item::action(ids::OPEN_SETTINGS, "Edit exclusions and settings\u{2026}"),
        ],
    ));
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
