//! Viewer-side read access. Opens the database read-only; decryption needs reconstructed keys,
//! verification of signatures needs the verifying key from the USB share file.

use crate::db::{read_keys, KeyRecord};
use crate::error::{Result, StoreError};
use hermes_crypto::legacy::LegacyContext;
use hermes_crypto::sign::{chain_next, verify_checkpoint, CheckpointBody, ZERO_CHAIN};
use hermes_crypto::{CryptoError, KeyId, Opener, RecordContext};
use rusqlite::{params_from_iter, types::Value, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;
use zeroize::Zeroizing;

/// A row exactly as stored: only `seq`, `ts_us` and `key_id` are readable without the keys.
/// (`legacy_*` are populated only for rows written before the metadata-encryption migration.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: u64,
    pub ts_us: i64,
    pub key_id: KeyId,
    /// 1 = legacy (pre-HPKE), 2 = HPKE record with encrypted envelope.
    pub fmt: u8,
    pub legacy_session_id: Option<i64>,
    pub legacy_kind: Option<u16>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub from_us: Option<i64>,
    pub to_us: Option<i64>,
    /// Keyset pagination: only events with seq greater (ascending) / smaller (descending) than this.
    pub after_seq: Option<u64>,
    pub limit: Option<u32>,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCheckpoint {
    pub first_seq: u64,
    pub last_seq: u64,
    pub signer_id: [u8; 16],
    pub prev_chain: [u8; 32],
    pub last_chain: [u8; 32],
    pub sig: [u8; 64],
}

/// Result of [`ArchiveReader::verify_integrity`]. See docs/SECURITY.md for what this can and cannot prove.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntegrityReport {
    pub checkpoints_total: u64,
    pub checkpoints_verified: u64,
    /// Checkpoints whose signature is invalid or whose signer is not trusted (last_seq).
    pub bad_signature: Vec<u64>,
    /// Signed ranges whose rows no longer hash to the signed chain head (modified/deleted/inserted rows).
    pub chain_mismatch: Vec<(u64, u64)>,
    /// Consecutive checkpoints that do not link (missing checkpoint or rows): (previous last_seq, next first_seq).
    pub link_breaks: Vec<(u64, u64)>,
    /// HPKE-format rows outside any validly signed range (e.g. the unsigned tail, injected rows).
    pub unsigned_rows: Vec<(u64, u64)>,
    /// Legacy rows (pre-signing) that can never be covered by a signature.
    pub legacy_unsigned_rows: u64,
}

impl IntegrityReport {
    /// True when no anomaly was found (legacy rows are reported separately and do not count).
    pub fn is_clean(&self) -> bool {
        self.bad_signature.is_empty()
            && self.chain_mismatch.is_empty()
            && self.link_breaks.is_empty()
            && self.unsigned_rows.is_empty()
    }
}

pub struct ArchiveReader {
    conn: Connection,
}

const EVENT_COLS: &str = "seq, ts_us, key_id, fmt, legacy_session_id, legacy_kind, payload";

fn row_to_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    let kid: Vec<u8> = r.get(2)?;
    Ok(StoredEvent {
        seq: r.get::<_, i64>(0)? as u64,
        ts_us: r.get(1)?,
        key_id: KeyId(kid.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?),
        fmt: r.get::<_, i64>(3)? as u8,
        legacy_session_id: r.get(4)?,
        legacy_kind: r.get::<_, Option<i64>>(5)?.map(|k| k as u16),
        payload: r.get(6)?,
    })
}

impl ArchiveReader {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA query_only = ON; PRAGMA trusted_schema = OFF;")?;
        Ok(ArchiveReader { conn })
    }

    pub fn keys(&self) -> Result<Vec<KeyRecord>> {
        read_keys(&self.conn)
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Value from the `meta` table (clean-shutdown flag etc.).
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// SQLite's own structural check ("ok" when fine). Reads the whole file: can take seconds on big archives.
    pub fn quick_check(&self) -> Result<String> {
        Ok(self.conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?)
    }

    /// (oldest, newest) event timestamps, if any events exist.
    pub fn time_range(&self) -> Result<Option<(i64, i64)>> {
        let r: (Option<i64>, Option<i64>) = self.conn.query_row("SELECT MIN(ts_us), MAX(ts_us) FROM events", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(match r {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        })
    }

    pub fn events(&self, q: &EventQuery) -> Result<Vec<StoredEvent>> {
        let mut sql = format!("SELECT {EVENT_COLS} FROM events WHERE 1=1");
        let mut args: Vec<Value> = vec![];
        if let Some(v) = q.from_us {
            sql.push_str(" AND ts_us >= ?");
            args.push(Value::Integer(v));
        }
        if let Some(v) = q.to_us {
            sql.push_str(" AND ts_us < ?");
            args.push(Value::Integer(v));
        }
        if let Some(v) = q.after_seq {
            sql.push_str(if q.descending {
                " AND seq < ?"
            } else {
                " AND seq > ?"
            });
            args.push(Value::Integer(v as i64));
        }
        sql.push_str(if q.descending {
            " ORDER BY seq DESC"
        } else {
            " ORDER BY seq ASC"
        });
        if let Some(l) = q.limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args), row_to_event)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn count_events(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    pub fn checkpoints(&self) -> Result<Vec<StoredCheckpoint>> {
        let mut st = self
            .conn
            .prepare("SELECT first_seq, last_seq, signer_id, prev_chain, last_chain, sig FROM checkpoints ORDER BY last_seq")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, Vec<u8>>(5)?,
            ))
        })?;
        let mut out = vec![];
        for r in rows {
            let (f, l, s, p, c, g) = r?;
            let bad = || StoreError::Corrupt("checkpoint field length".into());
            out.push(StoredCheckpoint {
                first_seq: f as u64,
                last_seq: l as u64,
                signer_id: s.try_into().map_err(|_| bad())?,
                prev_chain: p.try_into().map_err(|_| bad())?,
                last_chain: c.try_into().map_err(|_| bad())?,
                sig: g.try_into().map_err(|_| bad())?,
            });
        }
        Ok(out)
    }

    /// Sequence numbers missing between the first and last event. The collector assigns gap-free
    /// numbers, so any hole indicates deletion or loss during a repair.
    pub fn seq_gaps(&self) -> Result<Vec<(u64, u64)>> {
        let mut st = self.conn.prepare(
            "SELECT seq + 1 AS gap_start, (SELECT MIN(seq) FROM events e2 WHERE e2.seq > e1.seq) - 1 AS gap_end
             FROM events e1
             WHERE NOT EXISTS (SELECT 1 FROM events e2 WHERE e2.seq = e1.seq + 1)
               AND e1.seq < (SELECT MAX(seq) FROM events)",
        )?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Verifies signed checkpoints and recomputes the hash chain over the stored rows.
    /// `trusted` maps signer id -> Ed25519 verifying key, taken from the USB share file (NOT from
    /// this database, which an attacker may have edited).
    pub fn verify_integrity(
        &self,
        trusted: &HashMap<[u8; 16], [u8; 32]>,
    ) -> Result<IntegrityReport> {
        let mut rep = IntegrityReport::default();
        let cps = self.checkpoints()?;
        rep.checkpoints_total = cps.len() as u64;
        let mut verified_ranges: Vec<(u64, u64)> = vec![];
        let mut prev: Option<&StoredCheckpoint> = None;
        let mut st = self.conn.prepare("SELECT seq, ts_us, key_id, payload FROM events WHERE seq BETWEEN ?1 AND ?2 ORDER BY seq")?;
        for cp in &cps {
            let body = CheckpointBody {
                signer_id: cp.signer_id,
                first_seq: cp.first_seq,
                last_seq: cp.last_seq,
                prev_chain: cp.prev_chain,
                last_chain: cp.last_chain,
            };
            let sig_ok = trusted
                .get(&cp.signer_id)
                .map(|vk| verify_checkpoint(vk, &body, &cp.sig).is_ok())
                .unwrap_or(false);
            if !sig_ok {
                rep.bad_signature.push(cp.last_seq);
            } else {
                rep.checkpoints_verified += 1;
                verified_ranges.push((cp.first_seq, cp.last_seq));
                let mut chain = cp.prev_chain;
                let rows = st.query_map([cp.first_seq as i64, cp.last_seq as i64], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                })?;
                let mut expected = cp.first_seq;
                let mut contiguous = true;
                for row in rows {
                    let (seq, ts, kid, payload) = row?;
                    if seq as u64 != expected {
                        contiguous = false;
                    }
                    expected = seq as u64 + 1;
                    let kid: [u8; 16] = kid
                        .try_into()
                        .map_err(|_| StoreError::Corrupt("event key_id length".into()))?;
                    chain = chain_next(&chain, seq as u64, ts, &KeyId(kid), &payload);
                }
                if !contiguous || chain != cp.last_chain || expected != cp.last_seq + 1 {
                    rep.chain_mismatch.push((cp.first_seq, cp.last_seq));
                }
            }
            if let Some(p) = prev {
                // A checkpoint may restart the chain at ZERO only when it follows contiguously: that is how the
                // first v2 checkpoint after migrated legacy rows looks (docs/MIGRATION.md). Any other mismatch
                // (a deleted checkpoint or rows) is a break.
                let linked = p.last_chain == cp.prev_chain || cp.prev_chain == ZERO_CHAIN;
                if p.last_seq + 1 != cp.first_seq || !linked {
                    rep.link_breaks.push((p.last_seq, cp.first_seq));
                }
            }
            prev = Some(cp);
        }
        // HPKE rows not inside any validly signed range.
        let mut st = self
            .conn
            .prepare("SELECT seq FROM events WHERE fmt = 2 ORDER BY seq")?;
        let mut cur: Option<(u64, u64)> = None;
        let mut vi = 0usize;
        let rows = st.query_map([], |r| r.get::<_, i64>(0))?;
        for s in rows {
            let s = s? as u64;
            while vi < verified_ranges.len() && verified_ranges[vi].1 < s {
                vi += 1;
            }
            let covered = verified_ranges
                .get(vi)
                .map(|r| r.0 <= s && s <= r.1)
                .unwrap_or(false);
            if !covered {
                cur = match cur {
                    Some((a, b)) if b + 1 == s => Some((a, s)),
                    Some(done) => {
                        rep.unsigned_rows.push(done);
                        Some((s, s))
                    }
                    None => Some((s, s)),
                };
            }
        }
        if let Some(done) = cur {
            rep.unsigned_rows.push(done);
        }
        rep.legacy_unsigned_rows =
            self.conn
                .query_row("SELECT count(*) FROM events WHERE fmt = 1", [], |r| {
                    r.get::<_, i64>(0)
                })? as u64;
        Ok(rep)
    }
}

/// A decrypted event. For HPKE (fmt 2) rows kind and session id come from the encrypted envelope.
#[derive(Debug)]
pub struct DecryptedEvent {
    pub seq: u64,
    pub ts_us: i64,
    pub session_id: u64,
    pub kind: u16,
    pub body: Zeroizing<Vec<u8>>,
    /// Row was written by the pre-HPKE format (its session id / kind were stored in plaintext).
    pub legacy: bool,
}

/// The set of reconstructed keys a viewer session holds. Dropping it zeroizes the keys.
#[derive(Default)]
pub struct Keyring {
    openers: HashMap<KeyId, Opener>,
}

#[derive(Debug)]
pub enum OpenEventError {
    /// No share pair has been loaded for this key: show "locked – needs key X" in the UI.
    MissingKey(KeyId),
    /// Authentication failed: record damaged, tampered with, or moved to a different row.
    Tampered,
    Other(CryptoError),
}

impl Keyring {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, opener: Opener) {
        self.openers.insert(opener.key_id(), opener);
    }
    pub fn has(&self, id: &KeyId) -> bool {
        self.openers.contains_key(id)
    }
    pub fn len(&self) -> usize {
        self.openers.len()
    }
    pub fn is_empty(&self) -> bool {
        self.openers.is_empty()
    }

    pub fn open_event(
        &self,
        ev: &StoredEvent,
    ) -> std::result::Result<DecryptedEvent, OpenEventError> {
        let op = self
            .openers
            .get(&ev.key_id)
            .ok_or(OpenEventError::MissingKey(ev.key_id))?;
        let map = |e: CryptoError| match e {
            CryptoError::AuthenticationFailed
            | CryptoError::MalformedRecord
            | CryptoError::KeyIdMismatch { .. } => OpenEventError::Tampered,
            other => OpenEventError::Other(other),
        };
        match ev.fmt {
            2 => {
                let rec = op
                    .open_record(
                        &RecordContext {
                            seq: ev.seq,
                            ts_us: ev.ts_us,
                        },
                        &ev.payload,
                    )
                    .map_err(map)?;
                Ok(DecryptedEvent {
                    seq: ev.seq,
                    ts_us: ev.ts_us,
                    session_id: rec.session_id,
                    kind: rec.kind,
                    body: Zeroizing::new(rec.body),
                    legacy: false,
                })
            }
            1 => {
                let (sid, kind) = match (ev.legacy_session_id, ev.legacy_kind) {
                    (Some(s), Some(k)) => (s, k),
                    _ => return Err(OpenEventError::Tampered),
                };
                let ctx = LegacyContext {
                    seq: ev.seq,
                    ts_us: ev.ts_us,
                    session_id: sid,
                    kind,
                };
                let body = op.open_legacy_v1(&ctx, &ev.payload).map_err(map)?;
                Ok(DecryptedEvent {
                    seq: ev.seq,
                    ts_us: ev.ts_us,
                    session_id: sid as u64,
                    kind,
                    body,
                    legacy: true,
                })
            }
            other => Err(OpenEventError::Other(CryptoError::UnsupportedVersion(
                other,
            ))),
        }
    }
}
