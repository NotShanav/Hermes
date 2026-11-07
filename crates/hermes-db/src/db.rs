use crate::error::{is_corruption, Result, StoreError};
use crate::schema::{self, APPLICATION_ID};
use hermes_crypto::{KeyId, PublicKeyInfo};
use rusqlite::types::Value;
use rusqlite::{
    params, params_from_iter, Connection, OpenFlags, OptionalExtension, TransactionBehavior,
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub reason: String,
    pub quarantined_to: PathBuf,
    pub salvaged_keys: u64,
    pub salvaged_events: u64,
    pub salvaged_checkpoints: u64,
    /// false when reading the damaged file stopped early or skipped damaged pages.
    pub salvage_complete: bool,
}

#[derive(Debug, Default, Clone)]
pub struct OpenReport {
    pub recovery: Option<RecoveryReport>,
    /// The previous run did not call `close` (crash, power loss, kill). Meaningless for new files.
    pub unclean_previous_shutdown: bool,
    pub ran_integrity_check: bool,
    pub migrated_from: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenOptions {
    /// Always run `PRAGMA quick_check` regardless of the last-shutdown state.
    pub force_integrity_check: bool,
}

const RECHECK_INTERVAL_US: i64 = 24 * 3600 * 1_000_000;

pub struct Database {
    pub(crate) conn: Connection,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    pub info: PublicKeyInfo,
    /// Ed25519 verifying key registered with this key set (None for key sets from before signing).
    pub sign_public: Option<[u8; 32]>,
    pub created_us: i64,
    pub retired_us: Option<i64>,
    pub active: bool,
}

pub(crate) fn read_keys(conn: &Connection) -> Result<Vec<KeyRecord>> {
    let mut st = conn.prepare(
        "SELECT key_id, public_key, created_us, retired_us, status, sign_public FROM keys ORDER BY created_us, key_id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, Vec<u8>>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<Vec<u8>>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (kid, pk, created_us, retired_us, status, sp) = r?;
        let key_id = KeyId(
            kid.try_into()
                .map_err(|_| StoreError::Corrupt("key_id length".into()))?,
        );
        let public: [u8; 32] = pk
            .try_into()
            .map_err(|_| StoreError::Corrupt("public key length".into()))?;
        let sign_public = match sp {
            None => None,
            Some(v) => Some(
                <[u8; 32]>::try_from(v)
                    .map_err(|_| StoreError::Corrupt("sign key length".into()))?,
            ),
        };
        out.push(KeyRecord {
            info: PublicKeyInfo { key_id, public },
            sign_public,
            created_us,
            retired_us,
            active: status == "active",
        });
    }
    Ok(out)
}

impl Database {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens (creating if needed) the database. Corruption is handled by quarantining the
    /// damaged file and salvaging what can be read. Non-corruption errors (locked, i/o,
    /// newer schema, foreign database) are returned untouched and never destroy data.
    pub fn open(path: &Path, opts: OpenOptions) -> Result<(Database, OpenReport)> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        match Self::open_checked(path, opts, schema::MIGRATIONS) {
            Ok(r) => Ok(r),
            Err(StoreError::Sqlite(e)) if is_corruption(&e) => {
                Self::recover(path, format!("sqlite: {e}"))
            }
            Err(StoreError::Corrupt(why)) => Self::recover(path, why),
            Err(e) => Err(e),
        }
    }

    fn configure(conn: &Connection) -> rusqlite::Result<()> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA trusted_schema = OFF;
             PRAGMA cell_size_check = ON;
             PRAGMA synchronous = FULL;
             PRAGMA wal_autocheckpoint = 1000;
             PRAGMA journal_size_limit = 67108864;",
        )?;
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                Some(format!("could not enable WAL (got {mode})")),
            ));
        }
        Ok(())
    }

    pub(crate) fn open_checked(
        path: &Path,
        opts: OpenOptions,
        migrations: &[&str],
    ) -> Result<(Database, OpenReport)> {
        let conn = Connection::open(path)?;
        let mut report = OpenReport::default();

        // Identify the file with read-only pragmas BEFORE changing any setting: switching a
        // foreign database to WAL would rewrite its header, and we must never touch a database
        // that is not ours. (A damaged file fails right here with NotADatabase/Corrupt.)
        let app_id: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let fresh = version == 0 && app_id == 0 && {
            let n: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
            n == 0
        };
        if !fresh && app_id != APPLICATION_ID {
            return Err(StoreError::NotHermesDatabase(app_id));
        }
        if version > migrations.len() as u32 {
            return Err(StoreError::SchemaTooNew {
                found: version,
                supported: migrations.len() as u32,
            });
        }
        Self::configure(&conn)?;

        let mut db = Database {
            conn,
            path: path.to_path_buf(),
        };

        if !fresh {
            // Integrity check policy: after an unclean shutdown, once a day, or when forced.
            let clean = db.meta_get("clean_shutdown")?.as_deref() == Some("1");
            report.unclean_previous_shutdown = !clean;
            let last = db
                .meta_get("last_integrity_check_us")?
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            if opts.force_integrity_check || !clean || now_us() - last > RECHECK_INTERVAL_US {
                let res: String = db
                    .conn
                    .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
                if res != "ok" {
                    return Err(StoreError::Corrupt(format!("quick_check: {res}")));
                }
                db.meta_set("last_integrity_check_us", &now_us().to_string())?;
                report.ran_integrity_check = true;
            }
        }

        if version < migrations.len() as u32 {
            if !fresh {
                db.backup_before_migration(version)?;
                report.migrated_from = Some(version);
            }
            db.migrate(version, migrations)?;
            db.conn
                .pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        db.meta_set("clean_shutdown", "0")?;
        Ok((db, report))
    }

    fn backup_before_migration(&self, from: u32) -> Result<()> {
        let mut bak = self.path.clone().into_os_string();
        bak.push(format!(".pre-migration-v{from}.bak"));
        let bak = PathBuf::from(bak);
        let _ = std::fs::remove_file(&bak);
        self.conn
            .execute("VACUUM INTO ?1", [bak.to_string_lossy().as_ref()])?;
        Ok(())
    }

    fn migrate(&mut self, from: u32, migrations: &[&str]) -> Result<()> {
        for (i, sql) in migrations.iter().enumerate().skip(from as usize) {
            let version = (i + 1) as u32;
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(sql)
                .map_err(|source| StoreError::Migration { version, source })?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    fn has_meta(&self) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        if !self.has_meta()? {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        if !self.has_meta()? {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Marks the database as cleanly closed (skips the startup integrity scan next time).
    pub fn mark_clean_shutdown(&self) -> Result<()> {
        self.meta_set("clean_shutdown", "1")
    }

    // ---------------------------------------------------------------- keys

    /// Registers a public key (and, for signed key sets, its Ed25519 verifying key).
    pub fn register_key(
        &mut self,
        info: &PublicKeyInfo,
        sign_public: Option<&[u8; 32]>,
        make_active: bool,
    ) -> Result<()> {
        let now = now_us();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: i64 = tx.query_row(
            "SELECT count(*) FROM keys WHERE key_id = ?1",
            [&info.key_id.0[..]],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Err(StoreError::KeyExists(info.key_id.to_hex()));
        }
        let status = if make_active {
            tx.execute(
                "UPDATE keys SET status='retired', retired_us=?1 WHERE status='active'",
                [now],
            )?;
            "active"
        } else {
            "retired"
        };
        tx.execute(
            "INSERT INTO keys(key_id, public_key, created_us, retired_us, status, sign_public) VALUES (?1,?2,?3,?4,?5,?6)",
            params![&info.key_id.0[..], &info.public[..], now, if make_active { None } else { Some(now) }, status, sign_public.map(|s| s.to_vec())],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn keys(&self) -> Result<Vec<KeyRecord>> {
        read_keys(&self.conn)
    }

    pub fn active_key(&self) -> Result<Option<KeyRecord>> {
        Ok(self.keys()?.into_iter().find(|k| k.active))
    }

    /// Next sequence number. Never reuses a number the pre-repair database is known to have
    /// used (`min_next_seq`), so events lost in a repair stay visible as a gap.
    pub fn next_seq(&self) -> Result<u64> {
        let max: Option<i64> = self
            .conn
            .query_row("SELECT MAX(seq) FROM events", [], |r| r.get(0))?;
        let from_rows = max.map(|m| m as u64 + 1).unwrap_or(1);
        let floor = self
            .meta_get("min_next_seq")?
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);
        Ok(from_rows.max(floor))
    }

    /// Chain head of the newest checkpoint (None if no checkpoint exists yet).
    pub fn last_chain(&self) -> Result<Option<[u8; 32]>> {
        let v: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT last_chain FROM checkpoints ORDER BY last_seq DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match v {
            None => Ok(None),
            Some(b) => Ok(Some(
                <[u8; 32]>::try_from(b).map_err(|_| StoreError::Corrupt("chain length".into()))?,
            )),
        }
    }

    // ---------------------------------------------------------------- recovery

    fn recover(path: &Path, reason: String) -> Result<(Database, OpenReport)> {
        let ts = now_us() / 1000;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let quarantine = dir.join(format!("{name}.corrupt-{ts}"));
        // Move db + WAL + SHM aside as a unit (never delete: the user may want a manual repair).
        for suffix in ["", "-wal", "-shm"] {
            let from = dir.join(format!("{name}{suffix}"));
            if from.exists() {
                std::fs::rename(&from, dir.join(format!("{name}.corrupt-{ts}{suffix}")))?;
            }
        }
        let (mut db, mut report) =
            Self::open_checked(path, OpenOptions::default(), schema::MIGRATIONS)?;
        let mut rec = RecoveryReport {
            reason,
            quarantined_to: quarantine.clone(),
            salvaged_keys: 0,
            salvaged_events: 0,
            salvaged_checkpoints: 0,
            salvage_complete: true,
        };
        match Connection::open_with_flags(&quarantine, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(old) => db.salvage_from(&old, &mut rec),
            Err(_) => rec.salvage_complete = false,
        }
        report.unclean_previous_shutdown = true;
        report.recovery = Some(rec);
        Ok((db, report))
    }

    /// Best-effort copy of readable rows. Payloads stay opaque and are authenticated again when
    /// the viewer opens them, so damaged bytes are detected there; lost rows show up as sequence
    /// gaps and checkpoint/chain mismatches in the integrity report.
    fn salvage_from(&mut self, old: &Connection, rec: &mut RecoveryReport) {
        // Keep orphaned-but-readable encrypted rows rather than dropping them on FK checks.
        let _ = self.conn.execute_batch("PRAGMA foreign_keys = OFF;");
        // Highest sequence number the damaged file used (rightmost b-tree lookup; may fail).
        let old_max: Option<i64> = old
            .query_row("SELECT MAX(seq) FROM events", [], |r| r.get(0))
            .ok()
            .flatten();
        let old_version: Option<u32> = old.query_row("PRAGMA user_version", [], |r| r.get(0)).ok();
        match old_version {
            Some(v) if v >= 1 => self.salvage_tables(old, v, rec),
            _ => rec.salvage_complete = false,
        }
        if let Some(m) = old_max {
            let _ = self.meta_set("min_next_seq", &(m + 1).to_string());
        }
        let _ = self.conn.execute_batch("PRAGMA foreign_keys = ON;");
    }

    /// Copies a small table; returns (rows inserted, read completely).
    fn copy_small(
        &self,
        old: &Connection,
        select: &str,
        insert: &str,
        ncols: usize,
    ) -> (u64, bool) {
        let Ok(mut st) = old.prepare(select) else {
            return (0, false);
        };
        let Ok(rows) = st.query_map([], |r| {
            (0..ncols)
                .map(|i| r.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<Value>>>()
        }) else {
            return (0, false);
        };
        let mut n = 0;
        let mut complete = true;
        for row in rows {
            match row {
                Ok(v) => {
                    if let Ok(k) = self.conn.execute(insert, params_from_iter(v)) {
                        n += k as u64;
                    }
                }
                Err(_) => {
                    complete = false;
                    break;
                }
            }
        }
        (n, complete)
    }

    fn salvage_tables(&mut self, old: &Connection, version: u32, rec: &mut RecoveryReport) {
        // keys
        let (sel, cols) = if version >= 2 {
            (
                "SELECT key_id, public_key, created_us, retired_us, status, sign_public FROM keys",
                6,
            )
        } else {
            (
                "SELECT key_id, public_key, created_us, retired_us, status, NULL FROM keys",
                6,
            )
        };
        let (n, ok) = self.copy_small(
            old,
            sel,
            "INSERT OR IGNORE INTO keys(key_id,public_key,created_us,retired_us,status,sign_public) VALUES (?1,?2,?3,?4,?5,?6)",
            cols,
        );
        rec.salvaged_keys = n;
        rec.salvage_complete &= ok;
        // legacy sessions (v1 `sessions`, v2 `legacy_sessions`)
        let sess = if version >= 2 {
            "legacy_sessions"
        } else {
            "sessions"
        };
        let (_, ok) = self.copy_small(
            old,
            &format!("SELECT session_id, started_us, ended_us, end_reason FROM {sess}"),
            "INSERT OR IGNORE INTO legacy_sessions(session_id,started_us,ended_us,end_reason) VALUES (?1,?2,?3,?4)",
            4,
        );
        rec.salvage_complete &= ok;
        // checkpoints (v2 only)
        if version >= 2 {
            let (n, ok) = self.copy_small(
                old,
                "SELECT last_seq, first_seq, signer_id, prev_chain, last_chain, sig FROM checkpoints",
                "INSERT OR IGNORE INTO checkpoints(last_seq,first_seq,signer_id,prev_chain,last_chain,sig) VALUES (?1,?2,?3,?4,?5,?6)",
                6,
            );
            rec.salvaged_checkpoints = n;
            rec.salvage_complete &= ok;
        }
        // Events, in seq order, in chunks. A read error means a damaged page: keep the rows read
        // before it, step past the damaged region (sequence numbers are dense, so stepping by one
        // finds the next readable leaf) and continue. Give up after a long run of errors (e.g. a
        // destroyed interior page) and report the salvage as incomplete.
        let select = if version >= 2 {
            "SELECT seq, ts_us, key_id, fmt, legacy_session_id, legacy_kind, payload FROM events WHERE seq > ?1 ORDER BY seq LIMIT 256"
        } else {
            "SELECT seq, ts_us, key_id, 1, session_id, kind, payload FROM events WHERE seq > ?1 ORDER BY seq LIMIT 256"
        };
        let mut last: i64 = 0;
        let mut consecutive_errors: u32 = 0;
        loop {
            let mut rows: Vec<Vec<Value>> = Vec::new();
            let mut hit_error = false;
            {
                let Ok(mut st) = old.prepare(select) else {
                    rec.salvage_complete = false;
                    break;
                };
                let mapped = st.query_map([last], |r| {
                    (0..7)
                        .map(|i| r.get::<_, Value>(i))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                });
                match mapped {
                    Ok(it) => {
                        for r in it {
                            match r {
                                Ok(row) => rows.push(row),
                                Err(_) => {
                                    hit_error = true;
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => hit_error = true,
                };
            }
            let got = rows.len();
            if got > 0 {
                consecutive_errors = 0;
                let Ok(tx) = self.conn.transaction() else {
                    rec.salvage_complete = false;
                    break;
                };
                for r in rows {
                    if let Value::Integer(s) = r[0] {
                        last = s;
                    }
                    // Rows violating the schema CHECKs (e.g. mangled payload prefix) are skipped.
                    if let Ok(n) = tx.execute(
                        "INSERT OR IGNORE INTO events(seq,ts_us,key_id,fmt,legacy_session_id,legacy_kind,payload) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                        params_from_iter(r),
                    ) {
                        rec.salvaged_events += n as u64;
                    }
                }
                if tx.commit().is_err() {
                    rec.salvage_complete = false;
                    break;
                }
            }
            if hit_error {
                rec.salvage_complete = false;
                consecutive_errors += 1;
                if consecutive_errors > 20_000 {
                    break;
                }
                last += 1; // step past the unreadable row/page
            } else if got == 0 {
                break; // clean end of table
            }
        }
    }
}
