use hermes_crypto::protector::TestProtector;
use hermes_crypto::provision::{create_key_set, load_signer, NewKey};
use hermes_db::{Database, EventStore, OpenOptions, StoreConfig};
use hermes_tracker::*;
use hermes_viewer::*;
use std::path::PathBuf;

struct Env {
    d: tempfile::TempDir,
    _usb: tempfile::TempDir,
    prot: TestProtector,
    nk: NewKey,
    share_b: PathBuf,
}

fn new_env() -> Env {
    let d = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let prot = TestProtector::new();
    let share_b = usb.path().join("hermes-share-b.key");
    let nk = create_key_set(&d.path().join("keys"), &share_b, &prot).unwrap();
    let (mut db, _) = Database::open(&d.path().join("hermes.db"), OpenOptions::default()).unwrap();
    db.register_key(&nk.info, Some(&nk.sign_public), true).unwrap();
    db.mark_clean_shutdown().unwrap();
    Env { d, _usb: usb, prot, nk, share_b }
}

fn app(pid: u32, exe: Option<&str>, st: IdentityStatus) -> AppIdentity {
    AppIdentity { pid, process_start: Some(1), exe_path: exe.map(|e| format!("C:\\Apps\\{e}")), exe_name: exe.map(|e| e.to_string()), status: st, host_exe: None }
}
fn win(h: u64, title: Option<&str>) -> WindowInfo {
    WindowInfo { hwnd: h, class: Some("Cls".into()), title: title.map(|t| t.to_string()), title_status: if title.is_some() { TitleStatus::Text } else { TitleStatus::Redacted }, title_truncated: false }
}

const T0: i64 = 1_730_000_000_000_000; // 2024-10-27 03:33:20 UTC
const S: u64 = 0x1111;

fn record(env: &Env, evs: &[(i64, ActivityEvent)]) {
    let signer = load_signer(&env.d.path().join("keys"), &env.nk.info.key_id, &env.prot).unwrap();
    let mut st = EventStore::open(&env.d.path().join("hermes.db"), signer, StoreConfig::default()).unwrap();
    for (dt, e) in evs {
        st.push(S, T0 + dt, e.kind(), &encode_body(0, e)).unwrap();
    }
    st.close().unwrap();
}

fn sample(env: &Env) {
    record(
        env,
        &[
            (0, ActivityEvent::SessionStart { reason: StartReason::CollectorStart, version: "t".into() }),
            (1_000, ActivityEvent::FocusGained { app: app(10, Some("notepad.exe"), IdentityStatus::Resolved), window: win(1, Some("Secret plans.txt - Notepad")), from_hwnd: None }),
            (2_000_000, ActivityEvent::TitleChanged { hwnd: 1, title: Some("Budget 2025.txt - Notepad".into()), title_status: TitleStatus::Text, title_truncated: false }),
            (4_000_000, ActivityEvent::FocusLost { reason: LostReason::Switched, hwnd: 1, pid: 10, focus_us: 4_000_000 }),
            (4_000_001, ActivityEvent::FocusGained { app: app(20, Some("chrome.exe"), IdentityStatus::Resolved), window: win(2, Some("Inbox - Mail")), from_hwnd: Some(1) }),
            (9_000_000, ActivityEvent::FocusLost { reason: LostReason::Switched, hwnd: 2, pid: 20, focus_us: 5_000_000 }),
            (9_000_001, ActivityEvent::FocusGained { app: app(30, None, IdentityStatus::Excluded), window: win(3, None), from_hwnd: Some(2) }),
            (12_000_000, ActivityEvent::FocusLost { reason: LostReason::SessionEnd, hwnd: 3, pid: 30, focus_us: 3_000_000 }),
            (12_000_001, ActivityEvent::SessionEnd { reason: EndReason::Lock }),
        ],
    );
}

fn open(env: &Env) -> Result<Archive, String> {
    Archive::open(env.d.path(), &[env.share_b.clone()], &env.prot)
}

#[test]
fn unlocks_decodes_attributes_and_verifies() {
    let env = new_env();
    sample(&env);
    let a = open(&env).unwrap();
    assert_eq!(a.events.len(), 9);
    assert_eq!((a.registered_keys, a.unlocked_keys, a.locked, a.tampered, a.legacy_rows), (1, 1, 0, 0, 0));
    assert!(a.integrity.is_clean() && a.trust_anchors == 1 && a.seq_gaps.is_empty());
    let kinds: Vec<_> = a.events.iter().map(|e| e.kind).collect();
    assert_eq!(kinds[0], ViewKind::SessionStart);
    assert_eq!(*kinds.last().unwrap(), ViewKind::SessionEnd);
    // the title change and the focus-end are attributed to the right application via the window handle
    let tc = a.events.iter().find(|e| e.kind == ViewKind::TitleChanged).unwrap();
    assert_eq!((tc.app.as_str(), tc.pid, tc.title.as_str()), ("notepad.exe", 10, "Budget 2025.txt - Notepad"));
    let fl = a.events.iter().find(|e| e.kind == ViewKind::FocusLost && e.pid == 20).unwrap();
    assert_eq!(fl.app, "chrome.exe");
    // process identity and window title shown for focus events
    let g = &a.events[1];
    assert_eq!((g.app.as_str(), g.exe_path.as_str(), g.pid, g.title.as_str()), ("notepad.exe", "C:\\Apps\\notepad.exe", 10, "Secret plans.txt - Notepad"));
    // excluded application is shown as such, never named
    assert!(a.events.iter().any(|e| e.app == "<excluded application>"));
    assert!(a.events.windows(2).all(|w| w[0].seq < w[1].seq && w[0].ts_us <= w[1].ts_us), "chronological");
}

#[test]
fn filters_by_date_application_text_and_session() {
    let env = new_env();
    sample(&env);
    let a = open(&env).unwrap();
    assert_eq!(a.select(&Filter::default()).len(), 9);
    // application filter: case-insensitive substring; boundary events carry no application
    let f = Filter { app: Some("NOTE".into()), ..Default::default() };
    let r = a.select(&f);
    assert_eq!(r.len(), 3); // focus, title change, focus end
    assert!(r.iter().all(|e| e.app == "notepad.exe"));
    // text search covers titles (including the title *change*), paths, classes
    assert_eq!(a.select(&Filter { text: Some("budget 2025".into()), ..Default::default() }).len(), 1);
    assert!(a.select(&Filter { text: Some("c:\\apps\\chrome".into()), ..Default::default() }).len() >= 2);
    assert_eq!(a.select(&Filter { text: Some("no such text".into()), ..Default::default() }).len(), 0);
    // excluded content is not searchable (it was never recorded)
    assert!(a.select(&Filter { text: Some("keepass".into()), ..Default::default() }).is_empty());
    // date: the sample is on 2024-10-27 (UTC); at -05:00 it is still the 26th (22:33)
    let day = |d: &str, off: i32| date_range(d, off, T0).unwrap();
    let (from, to) = day("2024-10-27", 0).unwrap();
    assert_eq!(a.select(&Filter { from_us: Some(from), to_us: Some(to), ..Default::default() }).len(), 9);
    let (from, to) = day("2024-10-26", 0).unwrap();
    assert_eq!(a.select(&Filter { from_us: Some(from), to_us: Some(to), ..Default::default() }).len(), 0);
    let (from, to) = day("2024-10-26", -300).unwrap();
    assert_eq!(a.select(&Filter { from_us: Some(from), to_us: Some(to), ..Default::default() }).len(), 9);
    assert_eq!(a.select(&Filter { session: Some(S), ..Default::default() }).len(), 9);
    assert_eq!(a.select(&Filter { session: Some(S + 1), ..Default::default() }).len(), 0);
    // combined
    let f = Filter { app: Some("chrome".into()), text: Some("inbox".into()), ..Default::default() };
    assert_eq!(a.select(&f).len(), 1);
}

#[test]
fn summary_adds_up_time_per_application() {
    let env = new_env();
    sample(&env);
    let a = open(&env).unwrap();
    let s = a.summary(&Filter::default());
    let get = |n: &str| s.iter().find(|x| x.app == n).unwrap().clone();
    assert_eq!((get("notepad.exe").focus_us, get("notepad.exe").switches), (4_000_000, 1));
    assert_eq!(get("chrome.exe").focus_us, 5_000_000);
    assert_eq!(get("<excluded application>").focus_us, 3_000_000);
    assert_eq!(s[0].app, "chrome.exe", "sorted by time, longest first");
}

#[test]
fn both_shares_are_required_and_wrong_ones_are_refused() {
    let env = new_env();
    sample(&env);
    // no Share B at all
    let e = Archive::open(env.d.path(), &[], &env.prot).err().unwrap();
    assert!(e.contains("both shares are required"), "{e}");
    // Share B file that does not exist
    assert!(Archive::open(env.d.path(), &[env.d.path().join("missing.key")], &env.prot).is_err());
    // Share B of another key set
    let usb2 = tempfile::tempdir().unwrap();
    let other = create_key_set(&tempfile::tempdir().unwrap().path().join("k"), &usb2.path().join("o.key"), &env.prot).unwrap();
    let e = Archive::open(env.d.path(), &[usb2.path().join("o.key")], &env.prot).err().unwrap();
    assert!(e.contains("CANNOT unlock") && e.contains(&other.info.key_id.to_hex()), "{e}");
    // Share A missing from this PC (e.g. a different Windows profile)
    std::fs::remove_file(hermes_crypto::shares::share_a_path(&env.d.path().join("keys"), &env.nk.info.key_id)).unwrap();
    assert!(open(&env).is_err());
    // Share A bound to another user/machine (protector differs)
    let env2 = new_env();
    sample(&env2);
    assert!(Archive::open(env2.d.path(), &[env2.share_b.clone()], &TestProtector::new()).is_err());
}

#[test]
fn empty_archive_opens_gracefully() {
    let env = new_env();
    let a = open(&env).unwrap();
    assert!(a.events.is_empty() && a.integrity.is_clean() && a.seq_gaps.is_empty());
    assert!(a.select(&Filter::default()).is_empty() && a.summary(&Filter::default()).is_empty());
}

#[test]
fn missing_archive_gives_a_helpful_error() {
    let d = tempfile::tempdir().unwrap();
    let e = Archive::open(d.path(), &[], &TestProtector::new()).err().unwrap();
    assert!(e.contains("hermes init"), "{e}");
}

#[test]
fn tampering_is_shown_not_hidden() {
    let env = new_env();
    sample(&env);
    let raw = rusqlite_edit(&env, "UPDATE events SET ts_us = ts_us + 5 WHERE seq = 3; DELETE FROM events WHERE seq = 6;");
    let _ = raw;
    let a = open(&env).unwrap();
    assert!(a.tampered >= 1, "modified row must show as tampered");
    assert!(a.events.iter().any(|e| e.kind == ViewKind::Tampered));
    assert!(!a.integrity.is_clean());
    assert_eq!(a.seq_gaps, vec![(6, 6)]);
}

fn rusqlite_edit(env: &Env, sql: &str) {
    let c = rusqlite::Connection::open(env.d.path().join("hermes.db")).unwrap();
    c.execute_batch(sql).unwrap();
}

#[test]
fn second_key_set_stays_locked_until_its_usb_file_is_supplied() {
    let env = new_env();
    sample(&env);
    // rotate to a second key set and record more
    let usb2 = tempfile::tempdir().unwrap();
    let share_b2 = usb2.path().join("b2.key");
    let nk2 = create_key_set(&env.d.path().join("keys"), &share_b2, &env.prot).unwrap();
    {
        let (mut db, _) = Database::open(&env.d.path().join("hermes.db"), OpenOptions::default()).unwrap();
        db.register_key(&nk2.info, Some(&nk2.sign_public), true).unwrap();
    }
    let signer = load_signer(&env.d.path().join("keys"), &nk2.info.key_id, &env.prot).unwrap();
    let mut st = EventStore::open(&env.d.path().join("hermes.db"), signer, StoreConfig::default()).unwrap();
    let e = ActivityEvent::SessionStart { reason: StartReason::Resume, version: "t".into() };
    st.push(S, T0 + 20_000_000, e.kind(), &encode_body(0, &e)).unwrap();
    st.close().unwrap();

    let a = open(&env).unwrap(); // only the old USB file
    assert_eq!((a.registered_keys, a.unlocked_keys), (2, 1));
    assert!(a.locked >= 1 && a.events.iter().any(|e| e.kind == ViewKind::Locked));
    assert!(a.notes.iter().any(|n| n.contains("LOCKED")));
    let both = Archive::open(env.d.path(), &[env.share_b.clone(), share_b2], &env.prot).unwrap();
    assert_eq!((both.unlocked_keys, both.locked), (2, 0));
    assert!(both.integrity.is_clean() && both.trust_anchors == 2);
}
