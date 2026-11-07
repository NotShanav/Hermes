use crate::db::*;
use crate::error::StoreError;
use crate::reader::*;
use crate::schema::{self, kind};
use crate::store::*;
use hermes_crypto::legacy::{LegacyContext, LegacySealer};
use hermes_crypto::{
    generate_key_set, reconstruct_private_key, KeySet, Opener, RecordContext, RecordSigner, Sealer,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Temp dir; honours HERMES_TEST_DIR so the whole suite can run on NTFS / exFAT / FAT32 volumes.
fn tmp() -> tempfile::TempDir {
    match std::env::var_os("HERMES_TEST_DIR") {
        Some(d) => {
            std::fs::create_dir_all(&d).unwrap();
            tempfile::Builder::new()
                .prefix("hermes-test-")
                .tempdir_in(d)
                .unwrap()
        }
        None => tempfile::tempdir().unwrap(),
    }
}

fn db_path(dir: &Path) -> PathBuf {
    dir.join("hermes.db")
}

struct Env {
    ks: KeySet,
    seed: [u8; 32],
    path: PathBuf,
}

impl Env {
    fn signer(&self) -> RecordSigner {
        RecordSigner::from_seed(&self.seed)
    }
    fn open(&self, cfg: StoreConfig) -> EventStore {
        EventStore::open(&self.path, self.signer(), cfg).unwrap()
    }
    fn keyring(&self) -> Keyring {
        keyring_for(&[&self.ks])
    }
    fn trust(&self) -> HashMap<[u8; 16], [u8; 32]> {
        let s = self.signer();
        HashMap::from([(s.signer_id(), s.public())])
    }
}

fn keyring_for(sets: &[&KeySet]) -> Keyring {
    let mut k = Keyring::new();
    for ks in sets {
        k.add(Opener::new(
            reconstruct_private_key(&ks.share_a, &ks.share_b, &ks.info.key_id).unwrap(),
        ));
    }
    k
}

/// New database with one active, signed key set.
fn new_env(dir: &Path) -> Env {
    let p = db_path(dir);
    let (mut db, _) = Database::open(&p, OpenOptions::default()).unwrap();
    let ks = generate_key_set().unwrap();
    let (signer, seed) = RecordSigner::generate().unwrap();
    db.register_key(&ks.info, Some(&signer.public()), true)
        .unwrap();
    db.mark_clean_shutdown().unwrap();
    Env {
        ks,
        seed: *seed,
        path: p,
    }
}

fn small_cfg() -> StoreConfig {
    StoreConfig {
        max_buffered_events: 10,
        max_buffered_bytes: 1 << 20,
        flush_events: 5,
        flush_interval: Duration::from_millis(10),
    }
}

fn last_event(rd: &ArchiveReader) -> StoredEvent {
    rd.events(&EventQuery {
        descending: true,
        limit: Some(1),
        ..Default::default()
    })
    .unwrap()
    .remove(0)
}

// ============================================================ basics

#[test]
fn fresh_database_has_schema_identity_and_minimal_plaintext_columns() {
    let d = tmp();
    let (db, rep) = Database::open(&db_path(d.path()), OpenOptions::default()).unwrap();
    assert_eq!(db.schema_version().unwrap(), schema::LATEST_VERSION);
    let app: i64 = db
        .conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app, schema::APPLICATION_ID);
    let mode: String = db
        .conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let sync: i64 = db
        .conn
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sync, 2, "synchronous=FULL");
    assert!(rep.recovery.is_none() && !rep.unclean_previous_shutdown);
    // The complete list of columns of the events table: this IS the plaintext metadata.
    let mut st = db
        .conn
        .prepare("SELECT name FROM pragma_table_info('events') ORDER BY cid")
        .unwrap();
    let cols: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        cols,
        [
            "seq",
            "ts_us",
            "key_id",
            "fmt",
            "legacy_session_id",
            "legacy_kind",
            "payload"
        ]
    );
}

#[test]
fn store_refuses_to_run_without_active_signed_key_and_matching_signer() {
    let d = tmp();
    let p = db_path(d.path());
    drop(Database::open(&p, OpenOptions::default()).unwrap());
    let (s, _) = RecordSigner::generate().unwrap();
    assert!(matches!(
        EventStore::open(&p, s, StoreConfig::default()),
        Err(StoreError::NoActiveKey)
    ));

    let (mut db, _) = Database::open(&p, OpenOptions::default()).unwrap();
    let ks = generate_key_set().unwrap();
    db.register_key(&ks.info, None, true).unwrap(); // key set without a signing key (pre-signing era)
    drop(db);
    let (s, _) = RecordSigner::generate().unwrap();
    assert!(matches!(
        EventStore::open(&p, s, StoreConfig::default()),
        Err(StoreError::KeySetNotSigned)
    ));

    let d2 = tmp();
    let env = new_env(d2.path());
    let (wrong, _) = RecordSigner::generate().unwrap();
    assert!(matches!(
        EventStore::open(&env.path, wrong, StoreConfig::default()),
        Err(StoreError::SignerMismatch)
    ));
}

#[test]
fn schema_rejects_unsealed_or_inconsistent_rows() {
    let d = tmp();
    let env = new_env(d.path());
    let (db, _) = Database::open(&env.path, OpenOptions::default()).unwrap();
    let kid = env.ks.info.key_id.0.to_vec();
    let ins = |fmt: i64, ls: Option<i64>, lk: Option<i64>, payload: Vec<u8>| {
        db.conn.execute(
            "INSERT INTO events(seq,ts_us,key_id,fmt,legacy_session_id,legacy_kind,payload) VALUES (1,1,?1,?2,?3,?4,?5)",
            rusqlite::params![kid, fmt, ls, lk, payload],
        )
    };
    assert!(ins(
        2,
        None,
        None,
        b"plain text window title that is long enough to pass a length check....".to_vec()
    )
    .is_err());
    let mut fake = vec![2u8];
    fake.extend_from_slice(&[0u8; 60]);
    assert!(
        ins(2, Some(1), Some(1), fake.clone()).is_err(),
        "v2 rows must not carry plaintext legacy metadata"
    );
    assert!(
        ins(2, None, None, fake).is_ok(),
        "well-shaped sealed-looking row is accepted (tripwire only)"
    );
}

#[test]
fn push_flush_read_decrypt_roundtrip_in_order() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig::default());
    let sid = new_session_id();
    for i in 0..300u32 {
        st.push(
            sid,
            2_000 + i as i64,
            10 + (i % 3) as u16,
            format!("evt-{i}").as_bytes(),
        )
        .unwrap();
    }
    st.flush().unwrap();
    let rd = ArchiveReader::open(&env.path).unwrap();
    let evs = rd.events(&EventQuery::default()).unwrap();
    assert_eq!(evs.len(), 300);
    let kr = env.keyring();
    for (i, e) in evs.iter().enumerate() {
        assert_eq!(e.seq, i as u64 + 1, "gap-free ascending seq");
        assert_eq!(e.ts_us, 2_000 + i as i64);
        assert_eq!(e.fmt, 2);
        let de = kr.open_event(e).unwrap();
        assert_eq!(
            (de.session_id, de.kind, de.legacy),
            (sid, 10 + (i % 3) as u16, false)
        );
        assert_eq!(&**de.body, format!("evt-{i}").as_bytes());
    }
    // chronological access paths that need no keys
    let range = rd
        .events(&EventQuery {
            from_us: Some(2_100),
            to_us: Some(2_110),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(range.len(), 10);
    let page = rd
        .events(&EventQuery {
            after_seq: Some(290),
            limit: Some(100),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.len(), 10);
    assert_eq!(last_event(&rd).seq, 300);
    assert!(rd.seq_gaps().unwrap().is_empty());
    assert!(rd.verify_integrity(&env.trust()).unwrap().is_clean());
}

#[test]
fn no_plaintext_or_identifying_metadata_reaches_disk() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig::default());
    let sid: u64 = 0x1122_3344_5566_7788;
    let marker = "SUPER-SECRET-WINDOW-TITLE-9f3a";
    let app = "payroll-app.exe";
    for i in 0..50 {
        st.push(sid, i, 0x4D4D, format!("{app}|{marker} {i}").as_bytes())
            .unwrap();
    }
    st.flush().unwrap();
    let scan = |dir: &Path| {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().map(|x| x == "bak").unwrap_or(false) {
                continue;
            }
            let b = std::fs::read(&p).unwrap();
            for needle in [
                marker.as_bytes(),
                app.as_bytes(),
                &sid.to_le_bytes()[..],
                &sid.to_be_bytes()[..],
            ] {
                assert!(
                    !b.windows(needle.len()).any(|w| w == needle),
                    "{:?} found in {}",
                    String::from_utf8_lossy(needle),
                    p.display()
                );
            }
        }
    };
    scan(d.path()); // db + wal before checkpoint
    st.close().unwrap();
    scan(d.path());
}

#[test]
fn writes_are_batched_each_flush_adds_one_contiguous_signed_checkpoint() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig {
        flush_events: 256,
        ..StoreConfig::default()
    });
    let rd = ArchiveReader::open(&env.path).unwrap();
    for i in 0..255 {
        assert_eq!(st.push(7, i, 10, b"x").unwrap(), PushOutcome::Buffered);
    }
    assert_eq!(st.buffered_events(), 255);
    assert_eq!(rd.count_events().unwrap(), 0, "no per-event disk writes");
    assert_eq!(st.push(7, 255, 10, b"x").unwrap(), PushOutcome::Flushed);
    assert_eq!(rd.count_events().unwrap(), 256);
    for i in 0..10 {
        st.push(7, 1000 + i, 10, b"y").unwrap();
    }
    st.flush().unwrap();
    let cps = rd.checkpoints().unwrap();
    assert_eq!(
        cps.iter()
            .map(|c| (c.first_seq, c.last_seq))
            .collect::<Vec<_>>(),
        vec![(1, 256), (257, 266)]
    );
    assert_eq!(cps[1].prev_chain, cps[0].last_chain);
}

#[test]
fn tick_flushes_aged_buffer() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(small_cfg());
    st.push(7, 1, 10, b"a").unwrap();
    st.tick().unwrap();
    assert_eq!(st.buffered_events(), 1, "not aged yet");
    std::thread::sleep(Duration::from_millis(30));
    st.tick().unwrap();
    assert_eq!(st.buffered_events(), 0);
}

#[test]
fn full_buffer_drops_events_but_records_an_honest_gap() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(small_cfg());
    // Break the destination so every flush fails.
    let side = rusqlite::Connection::open(&env.path).unwrap();
    side.execute_batch("ALTER TABLE events RENAME TO events_hidden;")
        .unwrap();
    let mut dropped = 0;
    for i in 0..40 {
        if st.push(7, 100 + i, 10, format!("e{i}").as_bytes()).unwrap() == PushOutcome::Dropped {
            dropped += 1;
        }
        assert!(st.buffered_events() <= 10, "buffer must stay bounded");
    }
    assert_eq!((dropped, st.total_dropped()), (30, 30));
    side.execute_batch("ALTER TABLE events_hidden RENAME TO events;")
        .unwrap();
    st.push(7, 500, 10, b"after").unwrap();
    st.flush().unwrap();

    let rd = ArchiveReader::open(&env.path).unwrap();
    let kr = env.keyring();
    let evs: Vec<_> = rd
        .events(&EventQuery::default())
        .unwrap()
        .iter()
        .map(|e| kr.open_event(e).unwrap())
        .collect();
    let gaps: Vec<_> = evs.iter().filter(|e| e.kind == kind::SYSTEM_GAP).collect();
    assert_eq!(gaps.len(), 1);
    assert!(String::from_utf8_lossy(&gaps[0].body).starts_with("dropped=30 "));
    assert!(
        rd.seq_gaps().unwrap().is_empty(),
        "dropped events never consumed sequence numbers"
    );
    assert_eq!(&**evs.last().unwrap().body, b"after");
    assert!(rd.verify_integrity(&env.trust()).unwrap().is_clean());
}

#[test]
fn oversize_event_rejected() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig::default());
    let big = vec![0u8; hermes_crypto::record::MAX_BODY + 1];
    assert!(matches!(
        st.push(7, 1, 10, &big),
        Err(StoreError::EventTooLarge)
    ));
}

// ============================================================ crash safety

#[test]
fn unflushed_events_lost_on_crash_but_state_is_consistent_and_marked() {
    let d = tmp();
    let env = new_env(d.path());
    {
        let mut st = env.open(StoreConfig {
            flush_events: 100,
            ..Default::default()
        });
        for i in 0..150 {
            st.push(7, 1000 + i, 10, b"x").unwrap();
        }
        // 100 flushed, 50 buffered. Dropped without close(): models a crash (no Drop-flush exists).
    }
    let mut st = env.open(StoreConfig::default());
    let rep = st.open_report().clone();
    assert!(rep.unclean_previous_shutdown && rep.ran_integrity_check && rep.recovery.is_none());
    let rd = ArchiveReader::open(&env.path).unwrap();
    assert_eq!(
        rd.count_events().unwrap(),
        101,
        "100 durable events + 1 recovery marker"
    );
    let kr = env.keyring();
    let marker = kr.open_event(&last_event(&rd)).unwrap();
    assert_eq!(
        (marker.kind, marker.session_id),
        (kind::SYSTEM_RECOVERED, 0)
    );
    assert_eq!(&**marker.body, b"unclean_shutdown last_durable_seq=100");
    st.push(7, 2001, 10, b"y").unwrap();
    st.flush().unwrap();
    assert_eq!(
        last_event(&rd).seq,
        102,
        "seq continues without reuse or gap"
    );
    assert!(rd.seq_gaps().unwrap().is_empty());
    assert!(rd.verify_integrity(&env.trust()).unwrap().is_clean());
}

#[test]
fn clean_close_skips_integrity_scan_and_writes_no_marker() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig::default());
    st.push(7, 2, 10, b"x").unwrap();
    st.close().unwrap();
    let st2 = env.open(StoreConfig::default());
    assert!(!st2.open_report().ran_integrity_check && !st2.open_report().unclean_previous_shutdown);
    assert_eq!(
        ArchiveReader::open(&env.path)
            .unwrap()
            .count_events()
            .unwrap(),
        1
    );
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn from_hex(s: &str) -> [u8; 32] {
    let mut o = [0u8; 32];
    for i in 0..32 {
        o[i] = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    o
}

/// Child half of the kill test: writes forever until the parent kills it.
#[test]
fn crash_child() {
    let (Ok(path), Ok(seed)) = (
        std::env::var("HERMES_CRASH_DB"),
        std::env::var("HERMES_CRASH_SEED"),
    ) else {
        return;
    };
    let mut st = EventStore::open(
        Path::new(&path),
        RecordSigner::from_seed(&from_hex(&seed)),
        StoreConfig {
            flush_events: 25,
            ..Default::default()
        },
    )
    .unwrap();
    let mut i: i64 = 0;
    loop {
        i += 1;
        st.push(7, i, 10, format!("child-event-{i}").as_bytes())
            .unwrap();
    }
}

#[test]
fn sigkill_during_writes_never_corrupts_gaps_or_breaks_the_signed_chain() {
    for round in 0..6u64 {
        let d = tmp();
        let env = new_env(d.path());
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::crash_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HERMES_CRASH_DB", &env.path)
            .env("HERMES_CRASH_SEED", to_hex(&env.seed))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let rd = loop {
            std::thread::sleep(Duration::from_millis(5));
            if let Ok(rd) = ArchiveReader::open(&env.path) {
                if rd.count_events().unwrap_or(0) >= 200 {
                    break rd;
                }
            }
            assert!(child.try_wait().unwrap().is_none(), "child exited early");
        };
        std::thread::sleep(Duration::from_millis(round * 7));
        child.kill().unwrap();
        child.wait().unwrap();
        drop(rd);

        let (db, rep) = Database::open(
            &env.path,
            OpenOptions {
                force_integrity_check: true,
            },
        )
        .unwrap();
        assert!(
            rep.recovery.is_none(),
            "round {round}: database needed repair after SIGKILL: {rep:?}"
        );
        assert!(rep.unclean_previous_shutdown);
        drop(db);
        let rd = ArchiveReader::open(&env.path).unwrap();
        let evs = rd.events(&EventQuery::default()).unwrap();
        assert!(evs.len() >= 200);
        let kr = env.keyring();
        for (i, e) in evs.iter().enumerate() {
            assert_eq!(e.seq, i as u64 + 1, "round {round}: gap or reorder");
            assert_eq!(
                &**kr.open_event(e).unwrap().body,
                format!("child-event-{}", i + 1).as_bytes()
            );
        }
        let rep = rd.verify_integrity(&env.trust()).unwrap();
        assert!(rep.is_clean(), "round {round}: {rep:?}");
        assert_eq!(rep.checkpoints_verified, rep.checkpoints_total);
    }
}

// ============================================================ migrations

#[test]
fn migrations_upgrade_backup_rollback_and_downgrade_protection() {
    let d = tmp();
    let p = db_path(d.path());
    let m1 = schema::MIGRATION_1;
    let m2 = "ALTER TABLE sessions ADD COLUMN label TEXT;";
    let bad = "CREATE TABLE half_done(x); THIS IS NOT SQL;";

    let (db, _) = Database::open_checked(&p, OpenOptions::default(), &[m1]).unwrap();
    db.conn
        .execute("INSERT INTO sessions(started_us) VALUES (5)", [])
        .unwrap();
    db.mark_clean_shutdown().unwrap();
    drop(db);

    let (db, rep) = Database::open_checked(&p, OpenOptions::default(), &[m1, m2]).unwrap();
    assert_eq!(rep.migrated_from, Some(1));
    assert_eq!(db.schema_version().unwrap(), 2);
    let n: i64 = db
        .conn
        .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    assert!(d.path().join("hermes.db.pre-migration-v1.bak").exists());
    db.mark_clean_shutdown().unwrap();
    drop(db);

    let r = Database::open_checked(&p, OpenOptions::default(), &[m1, m2, bad]);
    assert!(matches!(r, Err(StoreError::Migration { version: 3, .. })));
    let raw = rusqlite::Connection::open(&p).unwrap();
    let v: u32 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 2);
    let t: i64 = raw
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='half_done'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(t, 0, "partial migration must not persist");
    drop(raw);

    let before = std::fs::read(&p).unwrap();
    assert!(matches!(
        Database::open_checked(&p, OpenOptions::default(), &[m1]),
        Err(StoreError::SchemaTooNew {
            found: 2,
            supported: 1
        })
    ));
    assert!(
        std::fs::read(&p).unwrap() == before,
        "refused database must be left byte-identical"
    );
}

/// Builds a database exactly as the previous milestone (schema v1, plaintext session/kind columns,
/// legacy X25519+HKDF+XChaCha20 records) would have, with real legacy ciphertext.
fn build_v1_database(p: &Path, ks: &KeySet, n: i64) {
    let (db, _) =
        Database::open_checked(p, OpenOptions::default(), &[schema::MIGRATION_1]).unwrap();
    let k = ks.info;
    db.conn
        .execute(
            "INSERT INTO keys(key_id,public_key,created_us,retired_us,status) VALUES (?1,?2,1,NULL,'active')",
            rusqlite::params![&k.key_id.0[..], &k.public[..]],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO sessions(session_id,started_us,ended_us,end_reason) VALUES (3,10,500,0)",
            [],
        )
        .unwrap();
    let sealer = LegacySealer::new(k).unwrap();
    for i in 1..=n {
        let ctx = LegacyContext {
            seq: i as u64,
            ts_us: 100 + i,
            session_id: 3,
            kind: 42,
        };
        let payload = sealer
            .seal(&ctx, format!("legacy-body-{i}").as_bytes())
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO events(seq,ts_us,session_id,kind,key_id,payload) VALUES (?1,?2,3,42,?3,?4)",
                rusqlite::params![i, 100 + i, &k.key_id.0[..], payload],
            )
            .unwrap();
    }
    db.mark_clean_shutdown().unwrap();
}

#[test]
fn v1_database_migrates_keeps_legacy_rows_readable_and_requires_a_signed_key_set_for_new_writes() {
    let d = tmp();
    let p = db_path(d.path());
    let old_ks = generate_key_set().unwrap();
    build_v1_database(&p, &old_ks, 25);

    let (db, rep) = Database::open(&p, OpenOptions::default()).unwrap();
    assert_eq!(rep.migrated_from, Some(1));
    assert!(d.path().join("hermes.db.pre-migration-v1.bak").exists());
    assert_eq!(db.schema_version().unwrap(), 2);
    let ls: i64 = db
        .conn
        .query_row("SELECT count(*) FROM legacy_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ls, 1, "v1 sessions table preserved as legacy_sessions");
    let key = db.active_key().unwrap().unwrap();
    assert_eq!(key.sign_public, None);
    drop(db);

    // legacy rows: still readable, still labelled legacy, plaintext session/kind preserved
    let rd = ArchiveReader::open(&p).unwrap();
    let kr = keyring_for(&[&old_ks]);
    let evs = rd.events(&EventQuery::default()).unwrap();
    assert_eq!(evs.len(), 25);
    for (i, e) in evs.iter().enumerate() {
        assert_eq!(
            (e.fmt, e.legacy_session_id, e.legacy_kind),
            (1, Some(3), Some(42))
        );
        let de = kr.open_event(e).unwrap();
        assert!(de.legacy);
        assert_eq!((de.session_id, de.kind), (3, 42));
        assert_eq!(&**de.body, format!("legacy-body-{}", i + 1).as_bytes());
    }
    // tampering with a legacy row's plaintext metadata is still detected (it was bound by the old AAD)
    let raw = rusqlite::Connection::open(&p).unwrap();
    raw.execute("UPDATE events SET legacy_kind = 43 WHERE seq = 5", [])
        .unwrap();
    let ev5 = rd
        .events(&EventQuery {
            after_seq: Some(4),
            limit: Some(1),
            ..Default::default()
        })
        .unwrap()
        .remove(0);
    assert!(matches!(kr.open_event(&ev5), Err(OpenEventError::Tampered)));
    raw.execute("UPDATE events SET legacy_kind = 42 WHERE seq = 5", [])
        .unwrap();
    drop(raw);

    // New collector cannot silently continue under an unsigned (pre-signing) key set.
    let (s, _) = RecordSigner::generate().unwrap();
    assert!(matches!(
        EventStore::open(&p, s, StoreConfig::default()),
        Err(StoreError::KeySetNotSigned)
    ));

    // Rotation to a signed key set: new rows are HPKE/encrypted-envelope, legacy rows stay readable.
    let new_ks = generate_key_set().unwrap();
    let (signer, seed) = RecordSigner::generate().unwrap();
    let (mut db, _) = Database::open(&p, OpenOptions::default()).unwrap();
    db.register_key(&new_ks.info, Some(&signer.public()), true)
        .unwrap();
    drop(db);
    let mut st =
        EventStore::open(&p, RecordSigner::from_seed(&seed), StoreConfig::default()).unwrap();
    for i in 0..10 {
        st.push(99, 1000 + i, 7, format!("new-{i}").as_bytes())
            .unwrap();
    }
    st.close().unwrap();
    let rd = ArchiveReader::open(&p).unwrap();
    let kr = keyring_for(&[&old_ks, &new_ks]);
    let evs = rd.events(&EventQuery::default()).unwrap();
    assert_eq!(
        evs.len(),
        25 + 1 + 10,
        "25 legacy + 1 unclean-shutdown marker (migration run never closed) + 10 new"
    );
    let fmts: Vec<u8> = evs.iter().map(|e| e.fmt).collect();
    assert_eq!(fmts.iter().filter(|f| **f == 1).count(), 25);
    for e in &evs {
        assert!(kr.open_event(e).is_ok());
    }
    let trust = HashMap::from([(signer.signer_id(), signer.public())]);
    let rep = rd.verify_integrity(&trust).unwrap();
    assert!(rep.is_clean(), "{rep:?}");
    assert_eq!(
        rep.legacy_unsigned_rows, 25,
        "legacy rows can never carry a signature"
    );
}

#[test]
fn foreign_sqlite_database_is_never_quarantined_or_modified() {
    let d = tmp();
    let p = db_path(d.path());
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("CREATE TABLE notes(x); INSERT INTO notes VALUES ('my precious data');")
            .unwrap();
    }
    let before = std::fs::read(&p).unwrap();
    assert!(matches!(
        Database::open(&p, OpenOptions::default()),
        Err(StoreError::NotHermesDatabase(_))
    ));
    assert!(
        std::fs::read(&p).unwrap() == before,
        "foreign database was modified"
    );
    let corrupts = std::fs::read_dir(d.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("corrupt")
        })
        .count();
    assert_eq!(corrupts, 0);
}

// ============================================================ corruption

#[test]
fn garbage_file_is_quarantined_and_fresh_database_created() {
    let d = tmp();
    let p = db_path(d.path());
    let junk: Vec<u8> = (0..8192u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    std::fs::write(&p, &junk).unwrap();
    let (mut db, rep) = Database::open(&p, OpenOptions::default()).unwrap();
    let rec = rep.recovery.expect("recovery expected");
    assert!(
        std::fs::read(&rec.quarantined_to).unwrap() == junk,
        "original bytes preserved"
    );
    assert_eq!(rec.salvaged_events, 0);
    let ks = generate_key_set().unwrap();
    db.register_key(&ks.info, None, true).unwrap();
    assert_eq!(db.schema_version().unwrap(), schema::LATEST_VERSION);
}

#[test]
fn damaged_pages_are_quarantined_salvaged_and_the_damage_stays_visible() {
    let d = tmp();
    let env = new_env(d.path());
    {
        let mut st = env.open(StoreConfig::default());
        for i in 0..4000 {
            st.push(
                7,
                i + 10,
                10,
                format!("payload-{i}-{}", "z".repeat(60)).as_bytes(),
            )
            .unwrap();
        }
        st.close().unwrap();
    }
    {
        use std::io::{Seek, SeekFrom, Write};
        let len = std::fs::metadata(&env.path).unwrap().len();
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&env.path)
            .unwrap();
        for pct in [45u64, 65, 85] {
            f.seek(SeekFrom::Start((len * pct / 100) / 4096 * 4096))
                .unwrap();
            f.write_all(&vec![0xEE; 4096]).unwrap();
        }
        f.sync_all().unwrap();
    }
    // A cleanly closed database is not scanned on every start (policy: unclean / daily / forced),
    // so ask for the check explicitly, as the collector's "verify now" action would.
    let mut st = EventStore::open_with(
        &env.path,
        env.signer(),
        StoreConfig::default(),
        OpenOptions {
            force_integrity_check: true,
        },
    )
    .unwrap();
    let rec = st
        .open_report()
        .recovery
        .clone()
        .expect("corruption must be detected");
    assert!(rec.quarantined_to.exists());
    assert_eq!(rec.salvaged_keys, 1);
    assert!(
        rec.salvaged_events > 3000,
        "salvage should continue past damaged pages: {rec:?}"
    );
    assert!(!rec.salvage_complete);
    assert!(rec.salvaged_checkpoints > 0);
    assert!(
        st.database()
            .active_key()
            .unwrap()
            .unwrap()
            .sign_public
            .is_some(),
        "signing key registration salvaged"
    );
    let rd = ArchiveReader::open(&env.path).unwrap();
    st.push(7, 100, 10, b"after repair").unwrap();
    st.flush().unwrap();
    assert_eq!(
        last_event(&rd).seq,
        4002,
        "4000 old seqs never reused; marker took 4001"
    );
    let kr = env.keyring();
    let all = rd.events(&EventQuery::default()).unwrap();
    let ok = all.iter().filter(|e| kr.open_event(e).is_ok()).count();
    assert_eq!(
        ok as u64,
        rec.salvaged_events + 2,
        "every salvaged row still authenticates (+ marker + new event)"
    );
    let marker = kr.open_event(&all[all.len() - 2]).unwrap();
    assert_eq!(marker.kind, kind::SYSTEM_RECOVERED);
    assert!(String::from_utf8_lossy(&marker.body)
        .contains("repaired_database=1 salvage_complete=false"));
    assert!(
        !rd.seq_gaps().unwrap().is_empty(),
        "lost rows are visible as sequence gaps"
    );
    let ir = rd.verify_integrity(&env.trust()).unwrap();
    assert!(
        !ir.is_clean(),
        "lost rows must break the chain report: {ir:?}"
    );
}

// ============================================================ authenticity & tamper detection

fn seeded(n: i64, flush: usize) -> (tempfile::TempDir, Env, EventStore) {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig {
        flush_events: flush,
        ..Default::default()
    });
    for i in 0..n {
        st.push(7, 100 + i, 10, format!("e{i}").as_bytes()).unwrap();
    }
    st.flush().unwrap();
    (d, env, st)
}

#[test]
fn in_database_tampering_is_detected_by_aead_and_by_signed_chain() {
    let (_d, env, st) = seeded(10, 3);
    let kr = env.keyring();
    let raw = rusqlite::Connection::open(&env.path).unwrap();
    let get = |seq: i64| -> Vec<u8> {
        raw.query_row("SELECT payload FROM events WHERE seq=?1", [seq], |r| {
            r.get(0)
        })
        .unwrap()
    };
    let (a, b) = (get(2), get(3));
    raw.execute("UPDATE events SET payload=?1 WHERE seq=2", [&b])
        .unwrap(); // swap two payloads
    raw.execute("UPDATE events SET payload=?1 WHERE seq=3", [&a])
        .unwrap();
    raw.execute("UPDATE events SET ts_us = ts_us + 1 WHERE seq=5", [])
        .unwrap(); // rewrite a timestamp
    raw.execute("DELETE FROM events WHERE seq=9", []).unwrap(); // delete one
    drop(st);

    let rd = ArchiveReader::open(&env.path).unwrap();
    let evs = rd.events(&EventQuery::default()).unwrap();
    let bad: Vec<u64> = evs
        .iter()
        .filter(|e| matches!(kr.open_event(e), Err(OpenEventError::Tampered)))
        .map(|e| e.seq)
        .collect();
    assert_eq!(bad, vec![2, 3, 5]);
    assert_eq!(rd.seq_gaps().unwrap(), vec![(9, 9)]);
    let rep = rd.verify_integrity(&env.trust()).unwrap();
    assert!(!rep.is_clean());
    // checkpoints are (1-3) (4-6) (7-9) (10): the first three ranges were modified
    assert_eq!(rep.chain_mismatch, vec![(1, 3), (4, 6), (7, 9)]);
    assert_eq!(rep.bad_signature, Vec::<u64>::new());
}

#[test]
fn forged_rows_made_with_the_public_key_are_not_trusted() {
    // The public key is stored in the DB: anyone can produce a record that DECRYPTS correctly.
    // Only the signature chain tells genuine rows from forgeries.
    let (_d, env, st) = seeded(6, 3);
    let kr = env.keyring();
    let raw = rusqlite::Connection::open(&env.path).unwrap();
    let forge = Sealer::new(env.ks.info).unwrap();
    let seq = 7u64;
    let ts = 999i64;
    let payload = forge
        .seal_record(&RecordContext { seq, ts_us: ts }, 5, 7, b"forged event")
        .unwrap();
    raw.execute(
        "INSERT INTO events(seq,ts_us,key_id,fmt,legacy_session_id,legacy_kind,payload) VALUES (?1,?2,?3,2,NULL,NULL,?4)",
        rusqlite::params![seq as i64, ts, &env.ks.info.key_id.0[..], payload],
    )
    .unwrap();
    drop(st);
    let rd = ArchiveReader::open(&env.path).unwrap();
    let forged = rd
        .events(&EventQuery {
            after_seq: Some(6),
            ..Default::default()
        })
        .unwrap()
        .remove(0);
    assert_eq!(
        &**kr.open_event(&forged).unwrap().body,
        b"forged event",
        "decrypts fine: encryption alone proves nothing"
    );
    let rep = rd.verify_integrity(&env.trust()).unwrap();
    assert_eq!(
        rep.unsigned_rows,
        vec![(7, 7)],
        "...but the signature layer flags it"
    );
    assert!(!rep.is_clean());
}

#[test]
fn signature_attacks_are_detected() {
    let (_d, env, st) = seeded(9, 3);
    drop(st);
    let rd = ArchiveReader::open(&env.path).unwrap();
    // wrong / swapped verifying key (e.g. attacker replaces the DB-stored key; the viewer uses the USB one)
    let (other, _) = RecordSigner::generate().unwrap();
    let wrong = HashMap::from([(other.signer_id(), other.public())]);
    let rep = rd.verify_integrity(&wrong).unwrap();
    assert_eq!(rep.bad_signature.len(), 3);
    assert_eq!(rep.checkpoints_verified, 0);
    assert!(!rep.unsigned_rows.is_empty());
    let raw = rusqlite::Connection::open(&env.path).unwrap();
    // flipped signature byte
    raw.execute(
        "UPDATE checkpoints SET sig = randomblob(64) WHERE last_seq = 6",
        [],
    )
    .unwrap();
    let rep = rd.verify_integrity(&env.trust()).unwrap();
    assert_eq!(rep.bad_signature, vec![6]);
    assert_eq!(rep.unsigned_rows, vec![(4, 6)]);
}

#[test]
fn deleted_middle_checkpoint_and_tail_are_reported() {
    let (_d, env, st) = seeded(9, 3);
    drop(st);
    let rd = ArchiveReader::open(&env.path).unwrap();
    let raw = rusqlite::Connection::open(&env.path).unwrap();
    raw.execute("DELETE FROM checkpoints WHERE last_seq = 6", [])
        .unwrap();
    let rep = rd.verify_integrity(&env.trust()).unwrap();
    assert_eq!(rep.link_breaks, vec![(3, 7)]);
    assert_eq!(rep.unsigned_rows, vec![(4, 6)]);
    raw.execute("DELETE FROM checkpoints WHERE last_seq = 9", [])
        .unwrap();
    let rep = rd.verify_integrity(&env.trust()).unwrap();
    assert_eq!(rep.unsigned_rows, vec![(4, 9)], "unsigned tail is reported");
}

#[test]
fn documented_limitation_truncation_to_an_older_checkpoint_is_not_detectable() {
    // Deleting the newest rows AND their checkpoints leaves a perfectly valid older archive.
    // Detecting this needs an external anchor (docs/SECURITY.md "Future work"). This test pins the
    // behaviour so nobody mistakes the chain for rollback protection.
    let (_d, env, st) = seeded(9, 3);
    drop(st);
    let raw = rusqlite::Connection::open(&env.path).unwrap();
    raw.execute("DELETE FROM events WHERE seq > 6", []).unwrap();
    raw.execute("DELETE FROM checkpoints WHERE last_seq > 6", [])
        .unwrap();
    let rd = ArchiveReader::open(&env.path).unwrap();
    assert!(rd.verify_integrity(&env.trust()).unwrap().is_clean());
}

// ============================================================ keys & readers

#[test]
fn rotation_in_a_running_collector_with_new_signer_and_locked_history() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig::default());
    st.push(7, 10, 10, b"old key era").unwrap();
    st.flush().unwrap();

    let ks2 = generate_key_set().unwrap();
    let (signer2, seed2) = RecordSigner::generate().unwrap();
    st.database_mut()
        .register_key(&ks2.info, Some(&signer2.public()), true)
        .unwrap();
    // wrong signer for the new active key is refused
    assert!(matches!(
        st.reload_active_key(env.signer()),
        Err(StoreError::SignerMismatch)
    ));
    st.reload_active_key(RecordSigner::from_seed(&seed2))
        .unwrap();
    st.push(7, 20, 10, b"new key era").unwrap();
    st.flush().unwrap();

    let keys = st.database().keys().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys.iter().filter(|k| k.active).count(), 1);
    assert!(keys
        .iter()
        .find(|k| k.info == env.ks.info)
        .unwrap()
        .retired_us
        .is_some());
    let ks3 = generate_key_set().unwrap();
    assert!(st
        .database()
        .conn
        .execute(
            "INSERT INTO keys(key_id,public_key,created_us,status) VALUES (?1,?2,0,'active')",
            rusqlite::params![&ks3.info.key_id.0[..], &ks3.info.public[..]]
        )
        .is_err());
    assert!(matches!(
        st.database_mut().register_key(&ks2.info, None, true),
        Err(StoreError::KeyExists(_))
    ));

    let rd = ArchiveReader::open(&env.path).unwrap();
    let evs = rd.events(&EventQuery::default()).unwrap();
    let only_old = keyring_for(&[&env.ks]);
    let only_new = keyring_for(&[&ks2]);
    let both = keyring_for(&[&env.ks, &ks2]);
    assert!(only_old.open_event(&evs[0]).is_ok());
    assert!(
        matches!(only_old.open_event(&evs[1]), Err(OpenEventError::MissingKey(id)) if id == ks2.info.key_id)
    );
    assert!(matches!(
        only_new.open_event(&evs[0]),
        Err(OpenEventError::MissingKey(_))
    ));
    assert_eq!(&**both.open_event(&evs[1]).unwrap().body, b"new key era");
    assert_eq!(rd.keys().unwrap().len(), 2);
    // Changing the key_id column of a row to another registered key: authentication fails.
    let raw = rusqlite::Connection::open(&env.path).unwrap();
    raw.execute(
        "UPDATE events SET key_id = ?1 WHERE seq = 1",
        [&ks2.info.key_id.0[..]],
    )
    .unwrap();
    let moved = rd
        .events(&EventQuery {
            limit: Some(1),
            ..Default::default()
        })
        .unwrap()
        .remove(0);
    assert!(matches!(
        both.open_event(&moved),
        Err(OpenEventError::Tampered)
    ));
    // signatures from both key sets verify with both verifying keys (as stored on the two USB files)
    raw.execute(
        "UPDATE events SET key_id = ?1 WHERE seq = 1",
        [&env.ks.info.key_id.0[..]],
    )
    .unwrap();
    let mut trust = env.trust();
    trust.insert(signer2.signer_id(), signer2.public());
    assert!(rd.verify_integrity(&trust).unwrap().is_clean());
}

#[test]
fn reader_sees_only_committed_rows_while_writer_is_open() {
    let d = tmp();
    let env = new_env(d.path());
    let mut st = env.open(StoreConfig {
        flush_events: 1000,
        ..Default::default()
    });
    let rd = ArchiveReader::open(&env.path).unwrap();
    st.push(7, 1, 10, b"a").unwrap();
    assert_eq!(rd.count_events().unwrap(), 0);
    st.flush().unwrap();
    assert_eq!(rd.count_events().unwrap(), 1);
    assert!(rd.events(&EventQuery::default()).is_ok());
    assert!(rusqlite::Connection::open_with_flags(
        &env.path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
    )
    .unwrap()
    .execute("DELETE FROM events", [])
    .is_err());
}
