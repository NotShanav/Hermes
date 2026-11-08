//! Turns the decrypted event stream into focus segments: "this window had focus from A to B".
//! A new segment starts at every focus change AND at every title change, so time can be attributed to
//! window titles as well as applications. All timestamps are UTC microseconds.

use hermes_viewer::{ViewEvent, ViewKind};

pub const US_HOUR: i64 = 3_600_000_000;
pub const US_DAY: i64 = 86_400_000_000;

#[derive(Clone, Debug)]
pub struct Seg {
    pub start_us: i64,
    pub end_us: i64,
    pub app: String,
    pub exe: String,
    pub title: String,
    /// Same value for every segment of one uninterrupted stay in one window (title changes keep it).
    pub focus: u64,
}

struct Open {
    focus: u64,
    hwnd: u64,
    app: String,
    exe: String,
    title: String,
    since: i64,
}

fn flush(o: &Open, end: i64, out: &mut Vec<Seg>) {
    if end > o.since {
        out.push(Seg { start_us: o.since, end_us: end, app: o.app.clone(), exe: o.exe.clone(), title: o.title.clone(), focus: o.focus });
    }
}

pub fn build_segments(events: &[ViewEvent]) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut open: Option<Open> = None;
    let mut next_focus = 0u64;
    for e in events {
        match e.kind {
            // Session boundaries and storage gap markers end whatever was open: nothing is assumed across them.
            ViewKind::SessionStart | ViewKind::SessionEnd | ViewKind::System => {
                if let Some(o) = open.take() {
                    flush(&o, e.ts_us, &mut out);
                }
            }
            ViewKind::FocusGained => {
                if let Some(o) = open.take() {
                    flush(&o, e.ts_us, &mut out);
                }
                next_focus += 1;
                open = Some(Open { focus: next_focus, hwnd: e.hwnd, app: e.app.clone(), exe: e.exe_path.clone(), title: e.title.clone(), since: e.ts_us });
            }
            ViewKind::TitleChanged => {
                if let Some(o) = open.as_mut() {
                    if o.hwnd == e.hwnd {
                        flush(&*o, e.ts_us, &mut out);
                        o.title = e.title.clone();
                        o.since = e.ts_us;
                    }
                }
            }
            ViewKind::FocusLost => {
                if let Some(o) = open.take() {
                    if o.hwnd == e.hwnd {
                        flush(&o, e.ts_us, &mut out);
                    } else {
                        open = Some(o);
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some(o), Some(last)) = (open.take(), events.last()) {
        flush(&o, last.ts_us, &mut out);
    }
    out
}

/// Calls `f(bucket, start, end)` for every piece of `[start, end)` that falls into one local-time bucket of
/// `unit` microseconds (an hour or a day). `bucket` counts units since the epoch in local time.
pub fn slices(start: i64, end: i64, off_min: i32, unit: i64, mut f: impl FnMut(i64, i64, i64)) {
    let off = off_min as i64 * 60_000_000;
    let mut s = start;
    while s < end {
        let b = (s + off).div_euclid(unit);
        let boundary = (b + 1) * unit - off;
        let e = end.min(boundary);
        f(b, s, e);
        s = e;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn ev(kind: ViewKind, ts_us: i64, hwnd: u64, app: &str, title: &str) -> ViewEvent {
        ViewEvent { seq: 0, ts_us, session_id: 1, kind, app: app.into(), exe_path: String::new(), pid: 1, hwnd, window_class: String::new(), title: title.into(), detail: String::new(), focus_us: 0 }
    }

    #[test]
    fn focus_and_title_changes_become_segments() {
        let s = 1_000_000;
        let events = vec![
            ev(ViewKind::SessionStart, 0, 0, "", ""),
            ev(ViewKind::FocusGained, 10 * s, 7, "a.exe", "one"),
            ev(ViewKind::TitleChanged, 20 * s, 7, "a.exe", "two"),
            ev(ViewKind::FocusLost, 30 * s, 7, "a.exe", ""),
            ev(ViewKind::FocusGained, 30 * s, 8, "b.exe", "b"),
            ev(ViewKind::SessionEnd, 50 * s, 0, "", ""),
        ];
        let segs = build_segments(&events);
        assert_eq!(segs.len(), 3);
        assert_eq!((segs[0].start_us, segs[0].end_us, segs[0].title.as_str()), (10 * s, 20 * s, "one"));
        assert_eq!((segs[1].start_us, segs[1].end_us, segs[1].title.as_str()), (20 * s, 30 * s, "two"));
        assert_eq!(segs[0].focus, segs[1].focus, "a title change stays in the same focus period");
        assert_ne!(segs[1].focus, segs[2].focus);
        assert_eq!((segs[2].app.as_str(), segs[2].end_us), ("b.exe", 50 * s), "session end closes the open window");
    }

    #[test]
    fn slices_split_at_local_midnight() {
        // +05:30: local midnight is 18:30 UTC the day before.
        let mut got = vec![];
        let start = 18 * US_HOUR; // 23:30 local on day 0
        let end = 20 * US_HOUR; // 01:30 local on day 1
        slices(start, end, 330, US_DAY, |d, a, b| got.push((d, b - a)));
        assert_eq!(got, vec![(0, US_HOUR / 2), (1, US_HOUR + US_HOUR / 2)]);
    }
}
