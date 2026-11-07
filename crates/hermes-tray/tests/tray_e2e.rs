//! Drives the REAL `hermes-tray` executable through its menu commands (WM_COMMAND with the same ids the
//! popup menu returns) and inspects the encrypted archive it wrote. Needs a desktop with a notification
//! area (the tray refuses to record without one), so it is #[ignore]d:
//!
//!   cargo test -p hermes-tray --test tray_e2e -- --ignored --test-threads=1 --nocapture
#![cfg(windows)]
#![allow(unsafe_code)]

use hermes_crypto::provision::create_key_set;
use hermes_db::{ArchiveReader, Database, OpenOptions};
use hermes_dump::{run as dump, DumpOptions};
use hermes_tray::menu::ids;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_COMMAND};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Env {
    _d: tempfile::TempDir,
    _usb: tempfile::TempDir,
    data: PathBuf,
    share_b: PathBuf,
}

fn setup() -> Env {
    let d = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let prot = hermes_platform::default_protector().expect("DPAPI");
    let share_b = usb.path().join("share-b.key");
    let nk = create_key_set(&d.path().join("keys"), &share_b, prot.as_ref()).unwrap();
    let (mut db, _) = Database::open(&d.path().join("hermes.db"), OpenOptions::default()).unwrap();
    db.register_key(&nk.info, Some(&nk.sign_public), true).unwrap();
    db.mark_clean_shutdown().unwrap();
    Env { data: d.path().to_path_buf(), _d: d, _usb: usb, share_b }
}

fn spawn(env: &Env, extra: &[&str]) -> Child {
    // (Under Wine the compile-time path is a Unix path: map it to the Z: drive.)
    let exe = env!("CARGO_BIN_EXE_hermes-tray");
    let exe = if exe.starts_with('/') { format!("Z:{}", exe.replace('/', "\\")) } else { exe.to_string() };
    Command::new(exe).arg("--data-dir").arg(&env.data).args(["--duration-secs", "90"]).args(extra).spawn().expect("spawn tray")
}

fn tracker_window(timeout: Duration) -> Option<HWND> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        let h = unsafe { FindWindowW(w!("HermesTrackerWindow"), None) }.unwrap_or_default();
        if !h.0.is_null() {
            return Some(h);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn click(h: HWND, id: u32) {
    unsafe {
        PostMessageW(Some(h), WM_COMMAND, WPARAM(id as usize), LPARAM(0)).unwrap();
    }
}

fn wait_exit(mut c: Child, secs: u64) -> i32 {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if let Some(s) = c.try_wait().unwrap() {
            return s.code().unwrap_or(-1);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = c.kill();
    panic!("tray did not exit after Exit command");
}

fn timeline(env: &Env) -> (String, hermes_dump::DumpSummary) {
    let prot = hermes_platform::default_protector().unwrap();
    let opts = DumpOptions { data_dir: env.data.clone(), share_b_files: vec![env.share_b.clone()], from_us: None, to_us: None, limit: None, verify: true };
    let mut out = Vec::new();
    let sum = dump(&opts, prot.as_ref(), &mut out).unwrap();
    (String::from_utf8(out).unwrap(), sum)
}

fn seq_of(text: &str, needle: &str) -> u64 {
    let line = text.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("`{needle}` not in timeline:\n{text}"));
    line[1..].split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
#[ignore = "needs a desktop with a notification area"]
fn menu_pause_resume_exit_are_recorded_durably_and_the_database_closes_cleanly() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let env = setup();
    let child = spawn(&env, &[]);
    let h = tracker_window(Duration::from_secs(20)).expect("tray window never appeared (icon refused? see fail-closed behaviour)");
    std::thread::sleep(Duration::from_millis(1200));
    click(h, ids::PAUSE_RESUME); // pause
    std::thread::sleep(Duration::from_millis(1500));
    // the pause marker must already be DURABLE (flushed at the boundary), while the tray is still running
    let rd = ArchiveReader::open(&env.data.join("hermes.db")).unwrap();
    assert!(rd.count_events().unwrap() >= 3, "pause was not flushed immediately");
    click(h, ids::PAUSE_RESUME); // resume
    std::thread::sleep(Duration::from_millis(1500));
    click(h, ids::EXIT);
    assert_eq!(wait_exit(child, 20), 0);

    let (text, sum) = timeline(&env);
    println!("{text}");
    let pause = seq_of(&text, "SESSION_END      reason=Pause");
    let resume = seq_of(&text, "SESSION_START    reason=ResumeCollection");
    let stop = seq_of(&text, "SESSION_END      reason=CollectorStop");
    assert!(pause < resume && resume < stop);
    assert_eq!(resume, pause + 1, "nothing is recorded while paused");
    assert!(text.contains("integrity verdict: CLEAN"), "{text}");
    assert_eq!((sum.locked, sum.tampered), (0, 0));
    // graceful shutdown marked the database clean
    let (_db, rep) = Database::open(&env.data.join("hermes.db"), OpenOptions::default()).unwrap();
    assert!(!rep.unclean_previous_shutdown);
}

#[test]
#[ignore = "needs a desktop with a notification area"]
fn starting_paused_records_nothing_until_the_user_resumes() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let env = setup();
    let child = spawn(&env, &["--paused"]);
    let h = tracker_window(Duration::from_secs(20)).expect("tray window");
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(ArchiveReader::open(&env.data.join("hermes.db")).unwrap().count_events().unwrap(), 0, "events recorded while paused");
    click(h, ids::PAUSE_RESUME);
    std::thread::sleep(Duration::from_millis(1000));
    click(h, ids::EXIT);
    assert_eq!(wait_exit(child, 20), 0);
    let (text, _) = timeline(&env);
    let first = text.lines().find(|l| l.starts_with('#') && l.contains("SESSION_")).unwrap();
    assert!(first.contains("SESSION_START    reason=ResumeCollection"), "first event must be the user's resume: {first}");
    assert!(text.contains("integrity verdict: CLEAN"));
}

#[test]
#[ignore = "needs a desktop with a notification area"]
fn exiting_during_a_timed_pause_leaves_no_marker_and_closes_cleanly() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let env = setup();
    let child = spawn(&env, &[]);
    let h = tracker_window(Duration::from_secs(20)).expect("tray window");
    std::thread::sleep(Duration::from_millis(800));
    click(h, ids::PAUSE_15);
    std::thread::sleep(Duration::from_millis(1200));
    click(h, ids::EXIT); // exit while a timed pause is pending: no leftover timers, clean stop
    assert_eq!(wait_exit(child, 20), 0);
    let (text, _) = timeline(&env);
    // While paused NOTHING is recorded, not even "collector stopped": the pause marker is the last record.
    assert!(text.contains("SESSION_END      reason=Pause"), "{text}");
    assert!(!text.contains("CollectorStop"), "a stop marker was written while paused:\n{text}");
    assert!(text.contains("integrity verdict: CLEAN"));
    let (_db, rep) = Database::open(&env.data.join("hermes.db"), OpenOptions::default()).unwrap();
    assert!(!rep.unclean_previous_shutdown, "exit while paused must still close the database cleanly");
}
