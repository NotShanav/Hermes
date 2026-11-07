use crate::model::ViewEvent;
use crate::time::{fmt_offset, fmt_time};
use serde_json::json;
use std::io::Write;

/// Spreadsheet programs execute cells that start with = + - @ (CSV injection): such cells are prefixed with `'`.
fn csv_cell(s: &str, free_text: bool) -> String {
    let s = if free_text && s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub const CSV_HEADER: &str = "seq,time_local,utc_offset,time_utc_us,event,session,application,executable_path,pid,window_handle,window_class,title,detail";

/// Writes PLAINTEXT. The caller must have told the user so.
pub fn export_csv(events: &[&ViewEvent], offset_min: i32, w: &mut dyn Write) -> std::io::Result<()> {
    writeln!(w, "{CSV_HEADER}")?;
    for e in events {
        // (text, is free text that may come from a window title and so needs formula defusing)
        let row = [
            (e.seq.to_string(), false),
            (fmt_time(e.ts_us, offset_min), false),
            (fmt_offset(offset_min), false),
            (e.ts_us.to_string(), false),
            (e.kind.label().to_string(), false),
            (format!("{:016x}", e.session_id), false),
            (e.app.clone(), true),
            (e.exe_path.clone(), true),
            (e.pid.to_string(), false),
            (format!("{:#x}", e.hwnd), false),
            (e.window_class.clone(), true),
            (e.title.clone(), true),
            (e.detail.clone(), true),
        ];
        writeln!(w, "{}", row.iter().map(|(c, ft)| csv_cell(c, *ft)).collect::<Vec<_>>().join(","))?;
    }
    Ok(())
}

/// Writes PLAINTEXT. One JSON array of objects.
pub fn export_json(events: &[&ViewEvent], offset_min: i32, w: &mut dyn Write) -> std::io::Result<()> {
    let arr: Vec<_> = events
        .iter()
        .map(|e| {
            json!({
                "seq": e.seq,
                "time_local": fmt_time(e.ts_us, offset_min),
                "utc_offset": fmt_offset(offset_min),
                "time_utc_us": e.ts_us,
                "event": e.kind.label(),
                "session": format!("{:016x}", e.session_id),
                "application": e.app,
                "executable_path": e.exe_path,
                "pid": e.pid,
                "window_handle": e.hwnd,
                "window_class": e.window_class,
                "title": e.title,
                "detail": e.detail,
            })
        })
        .collect();
    serde_json::to_writer_pretty(&mut *w, &arr)?;
    writeln!(w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ViewKind;

    fn ev(title: &str) -> ViewEvent {
        ViewEvent { seq: 1, ts_us: 1_730_000_000_000_000, session_id: 0xAB, kind: ViewKind::FocusGained, app: "a.exe".into(), exe_path: "C:\\a.exe".into(), pid: 7, hwnd: 255, window_class: "C".into(), title: title.into(), detail: "Resolved".into(), focus_us: 0 }
    }

    #[test]
    fn csv_quotes_commas_quotes_newlines_and_defuses_formulas() {
        let e = [ev("plain"), ev("a,b \"q\"\nline2"), ev("=HYPERLINK(\"http://evil\")"), ev("+1+1"), ev("@SUM(A1)")];
        let refs: Vec<&ViewEvent> = e.iter().collect();
        let mut out = Vec::new();
        export_csv(&refs, 0, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with(CSV_HEADER));
        assert!(s.contains("\"a,b \"\"q\"\"\nline2\""));
        assert!(s.contains("'=HYPERLINK(\"\"http://evil\"\")") || s.contains("\"'=HYPERLINK"), "{s}");
        assert!(s.contains(",'+1+1,") && s.contains(",'@SUM(A1),"));
        assert!(s.contains("2024-10-27 03:33:20,+00:00"));
    }

    #[test]
    fn json_is_valid_and_complete() {
        let e = [ev("T1"), ev("T\u{e9}\"2")];
        let refs: Vec<&ViewEvent> = e.iter().collect();
        let mut out = Vec::new();
        export_json(&refs, 330, &mut out).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[1]["title"], "T\u{e9}\"2");
        assert_eq!(v[0]["time_local"], "2024-10-27 09:03:20");
        assert_eq!(v[0]["utc_offset"], "+05:30");
        assert_eq!(v[0]["event"], "FOCUS");
    }
}
