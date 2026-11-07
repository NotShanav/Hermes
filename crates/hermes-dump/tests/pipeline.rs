//! End-to-end on any OS: fake OS observations -> engine -> encrypted signed store -> dump (needs both shares).
use hermes_crypto::protector::TestProtector;
use hermes_crypto::provision::create_key_set;
use hermes_db::{Database, EventStore, OpenOptions, StoreConfig};
use hermes_dump::{fmt_ts, run, DumpOptions};
use hermes_tracker::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Default)]
struct World {
    t: u64,
    fg: Option<u64>,
    wins: HashMap<u64, Focus>,
}
#[derive(Clone, Default)]
struct Fake(Rc<RefCell<World>>);
impl Platform for Fake {
    fn wall_us(&self) -> i64 {
        1_730_000_000_000_000 + self.0.borrow().t as i64
    }
    fn mono_us(&self) -> u64 {
        self.0.borrow().t
    }
    fn new_session_id(&self) -> u64 {
        0xABCDEF0123456789
    }
    fn foreground(&self) -> Option<u64> {
        self.0.borrow().fg
    }
    fn describe(&self, h: u64) -> Result<Focus, DescribeError> {
        self.0
            .borrow()
            .wins
            .get(&h)
            .cloned()
            .ok_or(DescribeError::WindowGone)
    }
}
fn win(h: u64, pid: u32, exe: &str, title: &str) -> Focus {
    Focus {
        app: AppIdentity {
            pid,
            process_start: Some(1),
            exe_path: Some(format!("C:\\{exe}")),
            exe_name: Some(exe.into()),
            status: IdentityStatus::Resolved,
            host_exe: None,
        },
        window: WindowInfo {
            hwnd: h,
            class: Some("Cls".into()),
            title: Some(title.into()),
            title_status: TitleStatus::Text,
            title_truncated: false,
        },
    }
}

#[test]
fn collector_pipeline_end_to_end_with_dump_and_verification() {
    let d = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let prot = TestProtector::new();
    let keys_dir = d.path().join("keys");
    let share_b = usb.path().join("share-b.key");

    // "keytool init"
    let nk = create_key_set(&keys_dir, &share_b, &prot).unwrap();
    let db_path = d.path().join("hermes.db");
    let (mut db, report) = Database::open(&db_path, OpenOptions::default()).unwrap();
    db.register_key(&nk.info, Some(&nk.sign_public), true)
        .unwrap();

    // "collector": public key + signing key only (no shares touched)
    let signer = hermes_crypto::provision::load_signer(&keys_dir, &nk.info.key_id, &prot).unwrap();
    let mut store = EventStore::from_database(db, report, signer, StoreConfig::default()).unwrap();
    let f = Fake::default();
    {
        let mut w = f.0.borrow_mut();
        w.wins
            .insert(1, win(1, 10, "notepad.exe", "Secret plans.txt - Notepad"));
        w.wins
            .insert(2, win(2, 20, "KeePassXC.exe", "Vault - KeePassXC"));
        w.wins.insert(3, win(3, 30, "chrome.exe", "Inbox"));
        w.fg = Some(1);
    }
    let mut eng = Engine::new(f.clone(), Config::default());
    let mut out = vec![];
    eng.start(&mut out);
    for (dt, obs) in [
        (2_000_000, Observation::Foreground(Some(2))),
        (3_000_000, Observation::Foreground(Some(3))),
        (1_000_000, Observation::Lock),
        (5_000_000, Observation::Unlock),
        (1_000_000, Observation::Stop),
    ] {
        f.0.borrow_mut().t += dt;
        if let Observation::Foreground(h) = obs {
            f.0.borrow_mut().fg = h;
        }
        eng.handle(obs, &mut out);
    }
    for e in &out {
        store.push(e.session_id, e.ts_us, e.kind, &e.body).unwrap();
    }
    store.close().unwrap();

    // The database alone reveals neither app names nor titles nor session ids.
    let raw = std::fs::read(&db_path).unwrap();
    for needle in [
        "notepad",
        "Secret plans",
        "chrome",
        "Inbox",
        "KeePass",
        "Vault",
    ] {
        assert!(
            !raw.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "{needle} visible in database file"
        );
    }

    // "dump": both shares required
    let opts = DumpOptions {
        data_dir: d.path().to_path_buf(),
        share_b_files: vec![share_b.clone()],
        from_us: None,
        to_us: None,
        limit: None,
        verify: true,
    };
    let mut text = Vec::new();
    let sum = run(&opts, &prot, &mut text).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(
        text.contains("notepad.exe")
            && text.contains("Secret plans.txt - Notepad")
            && text.contains("chrome.exe")
    );
    assert!(
        !text.contains("KeePass") && !text.contains("Vault"),
        "excluded application leaked into the archive:\n{text}"
    );
    assert!(
        text.contains("reason=Lock")
            && text.contains("reason=Unlock")
            && text.contains("reason=CollectorStop")
    );
    assert!(text.contains("integrity verdict: CLEAN"), "{text}");
    assert_eq!((sum.locked, sum.tampered), (0, 0));
    assert!(sum.integrity.unwrap().is_clean());

    // without Share B the history stays locked
    let opts2 = DumpOptions {
        share_b_files: vec![],
        verify: false,
        ..opts
    };
    let mut text2 = Vec::new();
    let sum2 = run(&opts2, &prot, &mut text2).unwrap();
    let text2 = String::from_utf8(text2).unwrap();
    assert_eq!(sum2.decrypted, 0);
    assert!(sum2.locked > 0 && text2.contains("LOCKED") && !text2.contains("notepad"));
}

#[test]
fn timestamp_formatting() {
    assert_eq!(fmt_ts(0), "1970-01-01 00:00:00.000000 UTC");
    assert_eq!(
        fmt_ts(1_730_000_000_123_456),
        "2024-10-27 03:33:20.123456 UTC"
    );
    assert_eq!(
        fmt_ts(951_782_400_000_000),
        "2000-02-29 00:00:00.000000 UTC"
    );
}
