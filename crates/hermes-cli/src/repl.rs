//! Interactive terminal viewer. Reads commands from any `BufRead`, writes to any `Write` (scriptable and testable).
//! Everything shown comes from memory; nothing is written to disk except an explicit `export`.

use hermes_viewer::{date_range, export_csv, export_json, fmt_offset, fmt_time, Archive, Filter, ViewEvent, ViewKind};
use std::io::{BufRead, Write};

pub struct ReplConfig {
    pub offset_min: i32,
    pub page: usize,
    pub now_us: i64,
}

pub const PLAINTEXT_WARNING: &str = "WARNING: the exported file is PLAINTEXT and not encrypted. It contains your application names and window titles. Protect it (or delete it) yourself.";

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

pub fn format_event(e: &ViewEvent, off: i32) -> String {
    let head = format!("#{:<6} {}  {:<13}", e.seq, fmt_time(e.ts_us, off), e.kind.label());
    let body = match e.kind {
        ViewKind::FocusGained => format!("{} (pid {})  \"{}\"  [{}]", e.app, e.pid, clip(&e.title, 100), e.detail),
        ViewKind::TitleChanged => format!("{}  \"{}\"", if e.app.is_empty() { "<unknown window>" } else { &e.app }, clip(&e.title, 100)),
        ViewKind::FocusLost => format!("{}  {}", if e.app.is_empty() { "<unknown window>" } else { &e.app }, e.detail),
        _ => clip(&e.detail, 140),
    };
    format!("{head} {body}")
}

pub fn integrity_summary(a: &Archive) -> (bool, String) {
    let r = &a.integrity;
    let mut problems = vec![];
    if a.tampered > 0 {
        problems.push(format!("{} record(s) failed authentication (damaged or modified)", a.tampered));
    }
    if !a.seq_gaps.is_empty() {
        problems.push(format!("missing sequence numbers {:?} (rows deleted or lost)", a.seq_gaps));
    }
    if !r.chain_mismatch.is_empty() {
        problems.push(format!("signed ranges that no longer match: {:?}", r.chain_mismatch));
    }
    if !r.bad_signature.is_empty() {
        problems.push(format!("checkpoints with invalid or untrusted signatures: {:?}", r.bad_signature));
    }
    if !r.link_breaks.is_empty() {
        problems.push(format!("broken checkpoint links: {:?}", r.link_breaks));
    }
    if !r.unsigned_rows.is_empty() {
        problems.push(format!("records not covered by any valid signature: {:?}", r.unsigned_rows));
    }
    let clean = problems.is_empty();
    let mut s = if clean {
        format!("CLEAN: {} of {} signed checkpoints verified; no gaps, no modified records", r.checkpoints_verified, r.checkpoints_total)
    } else {
        format!("PROBLEMS FOUND:\n    - {}", problems.join("\n    - "))
    };
    if a.trust_anchors == 0 && r.checkpoints_total > 0 {
        s.push_str("\n    (signatures could NOT be checked: this USB key file has no signing key: an older key set)");
        return (false, s);
    }
    if a.events.is_empty() {
        s.push_str("\n    (the archive is empty)");
    }
    (clean, s)
}

pub fn banner(a: &Archive, off: i32) -> String {
    let mut o = String::new();
    match (a.events.first(), a.events.last()) {
        (Some(f), Some(l)) => o.push_str(&format!("Hermes archive: {} events, {} .. {} (UTC{})\n", a.events.len(), fmt_time(f.ts_us, off), fmt_time(l.ts_us, off), fmt_offset(off))),
        _ => o.push_str("Hermes archive: EMPTY: nothing has been recorded yet (run `hermes start`).\n"),
    }
    for n in &a.notes {
        o.push_str(&format!("  {n}\n"));
    }
    o.push_str(&format!("Integrity: {}\n", integrity_summary(a).1));
    if a.legacy_rows > 0 {
        o.push_str(&format!("Note: {} legacy record(s) from an older Hermes format: unsigned, shown as LEGACY.\n", a.legacy_rows));
    }
    if a.locked > 0 {
        o.push_str(&format!("Note: {} record(s) belong to key sets that are still locked.\n", a.locked));
    }
    o.push_str("Not detectable: deletion of the NEWEST records, or restoring an older copy of the archive.\n");
    o
}

const HELP: &str = "\
Commands:
  list | next | prev | top | bottom     page through the current results
  date <day>                            today | yesterday | 2025-01-31 | 2025-01-01..2025-01-31 | all   (local time)
  app <text>                            only this application (substring of the program name) | app all
  find <text>                           search application, path, window class and titles | find clear
  session <hex id>                      only one session | session all
  filters | reset                       show / clear all filters
  summary                               time in focus per application for the current date/session filter
  verify | info                         integrity details / archive banner
  export csv|json <file> [force]        write the CURRENT results as PLAINTEXT
  quit
";

pub fn run<R: BufRead, W: Write>(a: &Archive, cfg: &ReplConfig, mut input: R, out: &mut W) -> std::io::Result<()> {
    let off = cfg.offset_min;
    let mut f = Filter::default();
    let mut results = a.select(&f);
    let mut pos = 0usize;
    let page = cfg.page.max(1);

    macro_rules! show {
        () => {{
            if results.is_empty() {
                writeln!(out, "(no matching events)")?;
            } else {
                let end = (pos + page).min(results.len());
                for e in &results[pos..end] {
                    writeln!(out, "{}", format_event(e, off))?;
                }
                writeln!(out, "-- {}-{} of {} (next / prev) --", pos + 1, end, results.len())?;
            }
        }};
    }
    macro_rules! refilter {
        () => {{
            results = a.select(&f);
            pos = 0;
            writeln!(out, "{} event(s) match.", results.len())?;
            show!();
        }};
    }

    writeln!(out, "{}", banner(a, off))?;
    writeln!(out, "Type `help` for commands.")?;
    show!();
    let mut line = String::new();
    loop {
        write!(out, "hermes> ")?;
        out.flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        let t = line.trim();
        let (cmd, rest) = t.split_once(char::is_whitespace).map(|(c, r)| (c, r.trim())).unwrap_or((t, ""));
        match cmd {
            "" => {}
            "help" | "?" => write!(out, "{HELP}")?,
            "quit" | "exit" | "q" => break,
            "list" | "l" => show!(),
            "next" | "n" => {
                if pos + page < results.len() {
                    pos += page;
                }
                show!();
            }
            "prev" | "p" => {
                pos = pos.saturating_sub(page);
                show!();
            }
            "top" => {
                pos = 0;
                show!();
            }
            "bottom" | "end" => {
                pos = if results.is_empty() { 0 } else { (results.len() - 1) / page * page };
                show!();
            }
            "date" => match date_range(rest, off, cfg.now_us) {
                Ok(r) => {
                    f.from_us = r.map(|x| x.0);
                    f.to_us = r.map(|x| x.1);
                    refilter!();
                }
                Err(e) => writeln!(out, "error: {e}")?,
            },
            "app" => {
                f.app = if rest.is_empty() || rest == "all" { None } else { Some(rest.to_string()) };
                refilter!();
            }
            "find" | "search" => {
                f.text = if rest.is_empty() || rest == "clear" { None } else { Some(rest.to_string()) };
                refilter!();
            }
            "session" => {
                if rest.is_empty() || rest == "all" {
                    f.session = None;
                    refilter!();
                } else if let Ok(s) = u64::from_str_radix(rest.trim_start_matches("0x"), 16) {
                    f.session = Some(s);
                    refilter!();
                } else {
                    writeln!(out, "error: session ids are hexadecimal (see the S: column of an export)")?;
                }
            }
            "reset" => {
                f = Filter::default();
                refilter!();
            }
            "filters" => {
                writeln!(out, "date: {}..{}  app: {:?}  text: {:?}  session: {:?}", f.from_us.map(|x| fmt_time(x, off)).unwrap_or("-".into()), f.to_us.map(|x| fmt_time(x, off)).unwrap_or("-".into()), f.app, f.text, f.session.map(|s| format!("{s:016x}")))?;
            }
            "summary" => {
                let s = a.summary(&f);
                if s.is_empty() {
                    writeln!(out, "(no focus data in the selected range)")?;
                }
                for x in s.iter().take(30) {
                    let secs = x.focus_us / 1_000_000;
                    writeln!(out, "{:>3}h{:02}m{:02}s  {:>5} switches  {}", secs / 3600, secs % 3600 / 60, secs % 60, x.switches, x.app)?;
                }
            }
            "verify" => writeln!(out, "Integrity: {}", integrity_summary(a).1)?,
            "info" => writeln!(out, "{}", banner(a, off))?,
            "export" => {
                let mut it = rest.split_whitespace();
                let (fmt, path) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
                let force = it.next() == Some("force");
                if (fmt != "csv" && fmt != "json") || path.is_empty() {
                    writeln!(out, "usage: export csv|json <file> [force]")?;
                } else {
                    match write_export(&results, fmt, path, off, force) {
                        Ok(n) => writeln!(out, "Wrote {n} event(s) to {path}\n{PLAINTEXT_WARNING}")?,
                        Err(e) => writeln!(out, "error: {e}")?,
                    }
                }
            }
            other => writeln!(out, "unknown command `{other}` (type help)")?,
        }
    }
    Ok(())
}

/// Writes the events as PLAINTEXT. Refuses to overwrite an existing file unless `force`.
pub fn write_export(events: &[&ViewEvent], fmt: &str, path: &str, off: i32, force: bool) -> Result<usize, String> {
    let mut oo = std::fs::OpenOptions::new();
    oo.write(true);
    if force {
        oo.create(true).truncate(true);
    } else {
        oo.create_new(true);
    }
    let mut f = oo.open(path).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { format!("{path} already exists (add `force` / --force to overwrite)") } else { format!("cannot write {path}: {e}") })?;
    let mut w = std::io::BufWriter::new(&mut f);
    match fmt {
        "csv" => export_csv(events, off, &mut w),
        _ => export_json(events, off, &mut w),
    }
    .and_then(|_| w.flush())
    .map_err(|e| e.to_string())?;
    Ok(events.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hermes_crypto::protector::TestProtector;
    use hermes_crypto::provision::{create_key_set, load_signer};
    use hermes_db::{Database, EventStore, OpenOptions, StoreConfig};
    use hermes_tracker::*;
    use std::io::Cursor;

    const T0: i64 = 1_730_000_000_000_000;

    fn archive() -> (tempfile::TempDir, Archive) {
        let d = tempfile::tempdir().unwrap();
        let usb = tempfile::tempdir().unwrap();
        let prot = TestProtector::new();
        let share_b = usb.path().join("b.key");
        let nk = create_key_set(&d.path().join("keys"), &share_b, &prot).unwrap();
        let (mut db, _) = Database::open(&d.path().join("hermes.db"), OpenOptions::default()).unwrap();
        db.register_key(&nk.info, Some(&nk.sign_public), true).unwrap();
        drop(db);
        let signer = load_signer(&d.path().join("keys"), &nk.info.key_id, &prot).unwrap();
        let mut st = EventStore::open(&d.path().join("hermes.db"), signer, StoreConfig::default()).unwrap();
        let app = |pid, n: &str| AppIdentity { pid, process_start: Some(1), exe_path: Some(format!("C:\\{n}")), exe_name: Some(n.into()), status: IdentityStatus::Resolved, host_exe: None };
        let win = |h, t: &str| WindowInfo { hwnd: h, class: Some("C".into()), title: Some(t.into()), title_status: TitleStatus::Text, title_truncated: false };
        let evs = [
            ActivityEvent::SessionStart { reason: StartReason::CollectorStart, version: "t".into() },
            ActivityEvent::FocusGained { app: app(1, "notepad.exe"), window: win(1, "Plans"), from_hwnd: None },
            ActivityEvent::FocusLost { reason: LostReason::Switched, hwnd: 1, pid: 1, focus_us: 3_000_000 },
            ActivityEvent::FocusGained { app: app(2, "chrome.exe"), window: win(2, "Inbox, \"urgent\""), from_hwnd: Some(1) },
            ActivityEvent::SessionEnd { reason: EndReason::Lock },
        ];
        for (i, e) in evs.iter().enumerate() {
            st.push(9, T0 + i as i64 * 1_000_000, e.kind(), &encode_body(0, e)).unwrap();
        }
        st.close().unwrap();
        let a = Archive::open(d.path(), &[share_b], &prot).unwrap();
        (d, a)
    }

    fn session(a: &Archive, script: &str) -> String {
        let mut out = Vec::new();
        run(a, &ReplConfig { offset_min: 0, page: 3, now_us: T0 }, Cursor::new(script.to_string()), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn banner_pages_filters_search_and_summary() {
        let (_d, a) = archive();
        let o = session(&a, "app note\nreset\nfind inbox\nfind clear\nnext\nprev\nsummary\nverify\nquit\n");
        assert!(o.contains("5 events") && o.contains("Integrity: CLEAN"), "{o}");
        assert!(o.contains("Not detectable: deletion of the NEWEST records"), "limitation must be stated");
        assert!(o.contains("1 event(s) match") || o.contains("2 event(s) match"));
        assert!(o.contains("notepad.exe (pid 1)") && o.contains("\"Plans\""));
        assert!(o.contains("chrome.exe (pid 2)") && o.contains("Inbox"));
        assert!(o.contains("-- 1-3 of 5") && o.contains("-- 4-5 of 5"), "paging: {o}");
        assert!(o.contains("SESSION START") && o.contains("SESSION END") && o.contains("Lock"));
        assert!(o.contains("0h00m03s") && o.contains("notepad.exe"), "summary: {o}");
    }

    #[test]
    fn date_filter_and_errors() {
        let (_d, a) = archive();
        let o = session(&a, "date 2024-10-26\ndate 2024-10-27\ndate nonsense\ndate all\nbogus\nquit\n");
        assert!(o.contains("0 event(s) match") && o.contains("(no matching events)"));
        assert!(o.contains("5 event(s) match"));
        assert!(o.contains("error: `nonsense` is not a date"));
        assert!(o.contains("unknown command `bogus`"));
    }

    #[test]
    fn export_from_the_viewer_is_explicit_plaintext_and_refuses_overwrite() {
        let (d, a) = archive();
        let csv = d.path().join("out.csv");
        let json = d.path().join("out.json");
        let o = session(&a, &format!("find inbox\nexport csv {p}\nexport csv {p}\nexport csv {p} force\nexport json {j}\nexport\nquit\n", p = csv.display(), j = json.display()));
        assert!(o.contains("PLAINTEXT"), "{o}");
        assert!(o.contains("already exists"), "second export must refuse to overwrite: {o}");
        let text = std::fs::read_to_string(&csv).unwrap();
        assert!(text.lines().count() == 2 && text.contains("\"Inbox, \"\"urgent\"\"\""), "{text}");
        assert_eq!(std::fs::read_to_string(&json).unwrap().matches("\"seq\"").count(), 1);
        assert!(o.contains("usage: export"));
    }

    #[test]
    fn empty_archive_is_handled() {
        let d = tempfile::tempdir().unwrap();
        let usb = tempfile::tempdir().unwrap();
        let prot = TestProtector::new();
        let share_b = usb.path().join("b.key");
        let nk = create_key_set(&d.path().join("keys"), &share_b, &prot).unwrap();
        let (mut db, _) = Database::open(&d.path().join("hermes.db"), OpenOptions::default()).unwrap();
        db.register_key(&nk.info, Some(&nk.sign_public), true).unwrap();
        drop(db);
        let a = Archive::open(d.path(), &[share_b], &prot).unwrap();
        let o = session(&a, "list\nsummary\nfind x\nverify\nquit\n");
        assert!(o.contains("EMPTY") && o.contains("(no matching events)") && o.contains("no focus data"), "{o}");
    }
}
