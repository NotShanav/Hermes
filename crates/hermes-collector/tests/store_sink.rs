//! The collector core on any OS: key set + signing key via a test protector, events through the sink.
use hermes_collector::{format_status, warnings, Collector};
use hermes_crypto::protector::TestProtector;
use hermes_crypto::provision::create_key_set;
use hermes_db::{ArchiveReader, Database, EventQuery, OpenOptions};
use hermes_tracker::{Emit, Sink};

fn setup() -> (tempfile::TempDir, tempfile::TempDir, TestProtector, hermes_crypto::provision::NewKey) {
    let d = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let prot = TestProtector::new();
    let nk = create_key_set(&d.path().join("keys"), &usb.path().join("b.key"), &prot).unwrap();
    let (mut db, _) = Database::open(&d.path().join("hermes.db"), OpenOptions::default()).unwrap();
    db.register_key(&nk.info, Some(&nk.sign_public), true).unwrap();
    db.mark_clean_shutdown().unwrap();
    (d, usb, prot, nk)
}

fn ev(i: i64) -> Emit {
    Emit { ts_us: 1_000 + i, session_id: 7, kind: 0x110, body: format!("event {i}").into_bytes() }
}

#[test]
fn records_flushes_reports_status_and_closes_cleanly() {
    let (d, _usb, prot, nk) = setup();
    let c = Collector::open(d.path(), &prot, 3).unwrap();
    let status = c.status();
    let mut sink = c.sink(true);
    for i in 0..10 {
        sink.emit(ev(i));
    }
    assert!(sink.has_pending());
    assert_eq!(status.borrow().events_recorded, 10);
    sink.flush();
    assert!(!sink.has_pending());
    assert!(status.borrow().last_flush_us > 0);
    let text = format_status(&status.borrow(), "Recording", true);
    assert!(text.contains("Recording") && text.contains("PUBLIC key") && text.contains("BOTH key shares") && text.contains("Start with Windows: on"));
    assert!(text.contains("No problems detected"), "{text}");
    assert!(text.contains(&nk.info.key_id.to_hex()[..12]));
    drop(sink);
    let info = c.finish().unwrap();
    assert_eq!(info.events_recorded, 10);
    let rd = ArchiveReader::open(&d.path().join("hermes.db")).unwrap();
    assert_eq!(rd.count_events().unwrap(), 10);
    // clean close: next open sees no unclean shutdown
    let c2 = Collector::open(d.path(), &prot, 0).unwrap();
    assert!(!c2.status().borrow().unclean_previous_shutdown);
    c2.finish().unwrap();
}

#[test]
fn prepare_exit_flushes_and_marks_clean_even_if_the_process_dies_afterwards() {
    let (d, _usb, prot, _nk) = setup();
    {
        let c = Collector::open(d.path(), &prot, 0).unwrap();
        let mut sink = c.sink(true);
        sink.emit(ev(1));
        sink.prepare_exit(); // what WM_ENDSESSION does
        // process is killed here: no finish(), no Drop-flush
        std::mem::forget(sink);
        std::mem::forget(c);
    }
    let rd = ArchiveReader::open(&d.path().join("hermes.db")).unwrap();
    assert_eq!(rd.count_events().unwrap(), 1, "event is durable");
    let c2 = Collector::open(d.path(), &prot, 0).unwrap();
    assert!(!c2.status().borrow().unclean_previous_shutdown, "shutdown was recorded as clean");
}

#[test]
fn storage_failure_surfaces_as_a_warning_and_recovers() {
    let (d, _usb, prot, _nk) = setup();
    let c = Collector::open(d.path(), &prot, 0).unwrap();
    let status = c.status();
    let mut sink = c.sink(true);
    let side = rusqlite_conn(d.path());
    side.execute_batch("ALTER TABLE events RENAME TO events_hidden;").unwrap();
    sink.emit(ev(1));
    sink.flush();
    assert!(status.borrow().last_error.is_some());
    assert!(!warnings(&status.borrow()).is_empty(), "tray must show a warning state");
    assert!(format_status(&status.borrow(), "Recording", false).contains("ATTENTION"));
    side.execute_batch("ALTER TABLE events_hidden RENAME TO events;").unwrap();
    sink.flush();
    assert!(status.borrow().last_error.is_none(), "cleared once writes succeed");
    drop(sink);
    c.finish().unwrap();
    assert_eq!(ArchiveReader::open(&d.path().join("hermes.db")).unwrap().events(&EventQuery::default()).unwrap().len(), 1);
}

fn rusqlite_conn(dir: &std::path::Path) -> rusqlite_shim::Conn {
    rusqlite_shim::Conn::open(&dir.join("hermes.db"))
}

mod rusqlite_shim {
    // hermes-db re-exports nothing from rusqlite; use the sqlite3 CLI-free path via Database's public API is
    // not possible for DDL, so talk to SQLite through hermes-db's own dependency.
    pub struct Conn(rusqlite::Connection);
    impl Conn {
        pub fn open(p: &std::path::Path) -> Self {
            Conn(rusqlite::Connection::open(p).unwrap())
        }
        pub fn execute_batch(&self, s: &str) -> rusqlite::Result<()> {
            self.0.execute_batch(s)
        }
    }
}

#[test]
fn refuses_to_open_without_a_signed_key_set() {
    let d = tempfile::tempdir().unwrap();
    let prot = TestProtector::new();
    let (db, _) = Database::open(&d.path().join("hermes.db"), OpenOptions::default()).unwrap();
    drop(db);
    let e = Collector::open(d.path(), &prot, 0).err().expect("must fail");
    assert!(e.contains("hermes-keytool init"), "{e}");
}

#[test]
fn unclean_previous_run_is_reported_to_the_user() {
    let (d, _usb, prot, _nk) = setup();
    {
        let c = Collector::open(d.path(), &prot, 0).unwrap();
        let mut sink = c.sink(true);
        sink.emit(ev(1));
        sink.flush();
        std::mem::forget(sink);
        std::mem::forget(c); // killed without finish()
    }
    let c = Collector::open(d.path(), &prot, 0).unwrap();
    let w = warnings(&c.status().borrow());
    assert!(w.iter().any(|x| x.contains("did not shut down cleanly")), "{w:?}");
    c.finish().unwrap();
}
