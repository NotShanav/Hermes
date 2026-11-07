//! Collector-side writer. Seals each event with the active *public* key immediately (so plaintext
//! never sits in the buffer), keeps a bounded buffer of ciphertext rows, and writes them in one
//! transaction together with a signed hash-chain checkpoint. Holds no private decryption key, no
//! share, and never reads share files. It does hold the Ed25519 *signing* key (see docs/SECURITY.md).

use crate::db::{now_us, Database, OpenOptions, OpenReport};
use crate::error::{Result, StoreError};
use crate::schema::kind;
use hermes_crypto::record::MAX_BODY;
use hermes_crypto::sign::{chain_next, CheckpointBody, ZERO_CHAIN};
use hermes_crypto::{KeyId, RecordContext, RecordSigner, Sealer};
use rusqlite::{params, TransactionBehavior};
use std::path::Path;
use std::time::{Duration, Instant};

/// Random non-zero session id (0 is reserved for storage-layer events). Session ids are encrypted
/// inside records, so they are random rather than a visible counter.
pub fn new_session_id() -> u64 {
    loop {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("OS RNG");
        let v = u64::from_le_bytes(b);
        if v != 0 {
            return v;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StoreConfig {
    /// Hard bound on buffered rows. When full (and flushing fails) new events are dropped.
    pub max_buffered_events: usize,
    /// Hard bound on buffered ciphertext bytes.
    pub max_buffered_bytes: usize,
    /// Flush as soon as this many rows are buffered.
    pub flush_events: usize,
    /// `tick()` flushes if the oldest buffered row is older than this.
    pub flush_interval: Duration,
}
impl Default for StoreConfig {
    fn default() -> Self {
        StoreConfig {
            max_buffered_events: 4096,
            max_buffered_bytes: 8 * 1024 * 1024,
            flush_events: 256,
            flush_interval: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// Buffered in memory; not yet durable.
    Buffered,
    /// Buffered and the buffer was flushed to disk as part of this call.
    Flushed,
    /// Buffer was full and could not be flushed: event discarded (a SYSTEM_GAP marker is written
    /// later so the archive honestly shows the hole).
    Dropped,
}

struct Row {
    seq: u64,
    ts_us: i64,
    key_id: [u8; 16],
    payload: Vec<u8>,
}

#[derive(Default)]
struct GapAccumulator {
    dropped: u64,
    first_us: i64,
    last_us: i64,
}

pub struct EventStore {
    db: Database,
    sealer: Sealer,
    signer: RecordSigner,
    next_seq: u64,
    chain: [u8; 32],
    buf: Vec<Row>,
    buf_bytes: usize,
    cfg: StoreConfig,
    gap: GapAccumulator,
    oldest_buffered: Option<Instant>,
    open_report: OpenReport,
    total_dropped: u64,
    last_flush_us: i64,
}

impl EventStore {
    /// Opens the database. Fails with `NoActiveKey` / `KeySetNotSigned` / `SignerMismatch` rather
    /// than ever storing unencrypted or unsigned data.
    pub fn open(path: &Path, signer: RecordSigner, cfg: StoreConfig) -> Result<EventStore> {
        Self::open_with(path, signer, cfg, OpenOptions::default())
    }

    pub fn open_with(
        path: &Path,
        signer: RecordSigner,
        cfg: StoreConfig,
        opts: OpenOptions,
    ) -> Result<EventStore> {
        let (db, report) = Database::open(path, opts)?;
        Self::from_database(db, report, signer, cfg)
    }

    pub fn from_database(
        db: Database,
        open_report: OpenReport,
        signer: RecordSigner,
        cfg: StoreConfig,
    ) -> Result<EventStore> {
        let (sealer, next_seq, chain) = Self::load_state(&db, &signer)?;
        let mut st = EventStore {
            db,
            sealer,
            signer,
            next_seq,
            chain,
            buf: Vec::with_capacity(cfg.flush_events.min(1024)),
            buf_bytes: 0,
            cfg,
            gap: GapAccumulator::default(),
            oldest_buffered: None,
            open_report,
            total_dropped: 0,
            last_flush_us: 0,
        };
        if st.open_report.unclean_previous_shutdown && st.next_seq > 1 {
            let mut text = format!("unclean_shutdown last_durable_seq={}", st.next_seq - 1);
            if let Some(r) = &st.open_report.recovery {
                text.push_str(&format!(
                    " repaired_database=1 salvage_complete={}",
                    r.salvage_complete
                ));
            }
            st.push(0, now_us(), kind::SYSTEM_RECOVERED, text.as_bytes())?;
            st.flush()?;
        }
        Ok(st)
    }

    fn load_state(db: &Database, signer: &RecordSigner) -> Result<(Sealer, u64, [u8; 32])> {
        let rec = db.active_key()?.ok_or(StoreError::NoActiveKey)?;
        let sp = rec.sign_public.ok_or(StoreError::KeySetNotSigned)?;
        if sp != signer.public() {
            return Err(StoreError::SignerMismatch);
        }
        Ok((
            Sealer::new(rec.info)?,
            db.next_seq()?,
            db.last_chain()?.unwrap_or(ZERO_CHAIN),
        ))
    }

    pub fn open_report(&self) -> &OpenReport {
        &self.open_report
    }
    pub fn database(&self) -> &Database {
        &self.db
    }
    pub fn database_mut(&mut self) -> &mut Database {
        &mut self.db
    }
    pub fn buffered_events(&self) -> usize {
        self.buf.len()
    }
    pub fn buffered_bytes(&self) -> usize {
        self.buf_bytes
    }
    pub fn total_dropped(&self) -> u64 {
        self.total_dropped
    }
    pub fn active_key_id(&self) -> KeyId {
        self.sealer.key_id()
    }
    /// Wall-clock time of the last successful flush (0 if none yet).
    pub fn last_flush_us(&self) -> i64 {
        self.last_flush_us
    }

    /// Switches to the (newly registered) active key set after a rotation. Pending events are
    /// flushed under the old key first. `signer` must be the new key set's signing key.
    pub fn reload_active_key(&mut self, signer: RecordSigner) -> Result<()> {
        self.flush()?;
        let (sealer, _, _) = Self::load_state(&self.db, &signer)?;
        self.sealer = sealer;
        self.signer = signer;
        Ok(())
    }

    pub fn push(
        &mut self,
        session_id: u64,
        ts_us: i64,
        kind: u16,
        body: &[u8],
    ) -> Result<PushOutcome> {
        if body.len() > MAX_BODY {
            return Err(StoreError::EventTooLarge);
        }
        let mut flushed = false;
        if self.is_full() {
            if self.flush().is_ok() {
                flushed = true;
            } else {
                self.record_drop(ts_us);
                return Ok(PushOutcome::Dropped);
            }
        }
        // If earlier events were dropped, write the honest marker before this event.
        if self.gap.dropped > 0 && !self.is_full() {
            let g = std::mem::take(&mut self.gap);
            let text = format!(
                "dropped={} first_us={} last_us={}",
                g.dropped, g.first_us, g.last_us
            );
            self.seal_and_buffer(session_id, g.last_us, kind::SYSTEM_GAP, text.as_bytes())?;
        }
        self.seal_and_buffer(session_id, ts_us, kind, body)?;
        if self.buf.len() >= self.cfg.flush_events && self.flush().is_ok() {
            flushed = true;
        }
        Ok(if flushed {
            PushOutcome::Flushed
        } else {
            PushOutcome::Buffered
        })
    }

    fn is_full(&self) -> bool {
        self.buf.len() >= self.cfg.max_buffered_events
            || self.buf_bytes >= self.cfg.max_buffered_bytes
    }

    fn record_drop(&mut self, ts_us: i64) {
        if self.gap.dropped == 0 {
            self.gap.first_us = ts_us;
        }
        self.gap.dropped += 1;
        self.gap.last_us = ts_us;
        self.total_dropped += 1;
    }

    fn seal_and_buffer(
        &mut self,
        session_id: u64,
        ts_us: i64,
        kind: u16,
        body: &[u8],
    ) -> Result<()> {
        let seq = self.next_seq;
        let payload =
            self.sealer
                .seal_record(&RecordContext { seq, ts_us }, kind, session_id, body)?;
        self.next_seq += 1;
        self.buf_bytes += payload.len() + 64;
        if self.buf.is_empty() {
            self.oldest_buffered = Some(Instant::now());
        }
        self.buf.push(Row {
            seq,
            ts_us,
            key_id: self.sealer.key_id().0,
            payload,
        });
        Ok(())
    }

    /// Writes all buffered rows plus one signed checkpoint in a single IMMEDIATE transaction
    /// (WAL, synchronous=FULL). On failure the buffer is kept intact for retry; nothing is
    /// half-written and the in-memory chain head is unchanged.
    pub fn flush(&mut self) -> Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let first_seq = self.buf[0].seq;
        let last_seq = self.buf[self.buf.len() - 1].seq;
        let mut chain = self.chain;
        for r in &self.buf {
            chain = chain_next(&chain, r.seq, r.ts_us, &KeyId(r.key_id), &r.payload);
        }
        let body = CheckpointBody {
            signer_id: self.signer.signer_id(),
            first_seq,
            last_seq,
            prev_chain: self.chain,
            last_chain: chain,
        };
        let sig = self.signer.sign_checkpoint(&body);
        {
            let tx = self
                .db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            {
                let mut st = tx.prepare_cached(
                    "INSERT INTO events(seq, ts_us, key_id, fmt, legacy_session_id, legacy_kind, payload) VALUES (?1,?2,?3,2,NULL,NULL,?4)",
                )?;
                for r in &self.buf {
                    st.execute(params![r.seq as i64, r.ts_us, &r.key_id[..], &r.payload])?;
                }
            }
            tx.execute(
                "INSERT INTO checkpoints(last_seq, first_seq, signer_id, prev_chain, last_chain, sig) VALUES (?1,?2,?3,?4,?5,?6)",
                params![last_seq as i64, first_seq as i64, &body.signer_id[..], &body.prev_chain[..], &body.last_chain[..], &sig[..]],
            )?;
            tx.commit()?;
        }
        self.chain = chain;
        self.buf.clear();
        self.buf.shrink_to(self.cfg.flush_events.max(16));
        self.buf_bytes = 0;
        self.oldest_buffered = None;
        self.last_flush_us = now_us();
        Ok(())
    }

    /// Call periodically (e.g. from a timer). Flushes if the buffer has aged out.
    pub fn tick(&mut self) -> Result<()> {
        if let Some(t) = self.oldest_buffered {
            if t.elapsed() >= self.cfg.flush_interval {
                self.flush()?;
            }
        }
        Ok(())
    }

    /// Flush and mark the database cleanly closed (skips the startup integrity scan next time).
    pub fn close(mut self) -> Result<()> {
        self.flush()?;
        self.db.mark_clean_shutdown()?;
        Ok(())
    }
}
