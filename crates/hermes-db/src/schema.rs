//! Schema + forward-only migrations. `PRAGMA user_version` holds the applied version.
//!
//! Timestamps are microseconds since the Unix epoch (UTC). `seq` is a gap-free, strictly
//! increasing event number assigned by the collector; it defines the authoritative ordering
//! (wall clocks can step backwards). Everything about *what* happened (application, window,
//! text, ...) lives inside the encrypted `payload`; only coarse metadata is plaintext, see
//! docs/SECURITY.md for the trade-off.

/// Value of `PRAGMA application_id`: ASCII "HRMS".
pub const APPLICATION_ID: i64 = 0x4852_4D53;

pub const MIGRATION_1: &str = r#"
CREATE TABLE meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

-- Public keys only. Private keys / shares are never stored in the database.
CREATE TABLE keys (
    key_id     BLOB PRIMARY KEY NOT NULL CHECK (length(key_id) = 16),
    public_key BLOB NOT NULL CHECK (length(public_key) = 32),
    created_us INTEGER NOT NULL,
    retired_us INTEGER,
    status     TEXT NOT NULL CHECK (status IN ('active','retired'))
) STRICT;
-- At most one active key at any time.
CREATE UNIQUE INDEX keys_one_active ON keys(status) WHERE status = 'active';

CREATE TABLE sessions (
    session_id INTEGER PRIMARY KEY NOT NULL,
    started_us INTEGER NOT NULL,
    ended_us   INTEGER,
    end_reason INTEGER            -- NULL while open; see session_end::*
) STRICT;
CREATE INDEX sessions_started ON sessions(started_us);

CREATE TABLE events (
    seq        INTEGER PRIMARY KEY NOT NULL,
    ts_us      INTEGER NOT NULL,
    session_id INTEGER NOT NULL REFERENCES sessions(session_id),
    kind       INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 65535),
    key_id     BLOB NOT NULL REFERENCES keys(key_id),
    payload    BLOB NOT NULL,
    -- Defence in depth against accidental plaintext storage: payload must look like a sealed
    -- record (version byte 1, embedded key id equals key_id, header + tag present).
    -- This is a tripwire, NOT a security control.
    CHECK (length(payload) >= 89
           AND hex(substr(payload, 1, 1)) = '01'
           AND substr(payload, 2, 16) = key_id)
) STRICT;
CREATE INDEX events_ts      ON events(ts_us);
CREATE INDEX events_session ON events(session_id, seq);
CREATE INDEX events_kind_ts ON events(kind, ts_us);
"#;

/// v1 -> v2 (Beta 0.1 milestone "HPKE + metadata encryption + signed checkpoints").
///
/// * `events` loses the plaintext `session_id` and `kind` columns for NEW rows: both now live inside
///   the encrypted record envelope. Rows written under v1 are preserved verbatim as `fmt = 1`
///   ("legacy"): their session id / kind stay in `legacy_session_id` / `legacy_kind` and their
///   payload keeps the old encryption (still readable). They remain plaintext-labelled until
///   re-encrypted (not implemented yet, see docs/MIGRATION.md).
/// * The v1 `sessions` table is kept as `legacy_sessions`; new sessions are encrypted records.
/// * `keys` gains `sign_public` (Ed25519 verifying key; informational, the trust anchor is Share B).
/// * `checkpoints`: Ed25519-signed hash-chain heads, one per flush.
pub const MIGRATION_2: &str = r#"
ALTER TABLE sessions RENAME TO legacy_sessions;
DROP INDEX IF EXISTS sessions_started;
CREATE TABLE events_v2 (
    seq               INTEGER PRIMARY KEY NOT NULL,
    ts_us             INTEGER NOT NULL,
    key_id            BLOB NOT NULL REFERENCES keys(key_id),
    fmt               INTEGER NOT NULL CHECK (fmt IN (1, 2)),
    legacy_session_id INTEGER,
    legacy_kind       INTEGER,
    payload           BLOB NOT NULL,
    -- Tripwires against accidental plaintext storage (NOT security controls):
    CHECK (
      (fmt = 2 AND legacy_session_id IS NULL AND legacy_kind IS NULL
         AND length(payload) >= 49 AND hex(substr(payload, 1, 1)) = '02')
      OR
      (fmt = 1 AND legacy_session_id IS NOT NULL AND legacy_kind IS NOT NULL
         AND length(payload) >= 89 AND hex(substr(payload, 1, 1)) = '01'
         AND substr(payload, 2, 16) = key_id)
    )
) STRICT;
INSERT INTO events_v2(seq, ts_us, key_id, fmt, legacy_session_id, legacy_kind, payload)
    SELECT seq, ts_us, key_id, 1, session_id, kind, payload FROM events;
DROP TABLE events;
ALTER TABLE events_v2 RENAME TO events;
CREATE INDEX events_ts ON events(ts_us);
ALTER TABLE keys ADD COLUMN sign_public BLOB;
CREATE TABLE checkpoints (
    last_seq   INTEGER PRIMARY KEY NOT NULL,
    first_seq  INTEGER NOT NULL,
    signer_id  BLOB NOT NULL CHECK (length(signer_id) = 16),
    prev_chain BLOB NOT NULL CHECK (length(prev_chain) = 32),
    last_chain BLOB NOT NULL CHECK (length(last_chain) = 32),
    sig        BLOB NOT NULL CHECK (length(sig) = 64),
    CHECK (first_seq <= last_seq)
) STRICT;
"#;

/// Ordered list; index `i` migrates version `i` -> `i+1`.
pub const MIGRATIONS: &[&str] = &[MIGRATION_1, MIGRATION_2];

pub const LATEST_VERSION: u32 = MIGRATIONS.len() as u32;

/// Event kinds 0..=255 are reserved for the storage layer itself. (Kinds are encrypted: they are
/// visible only inside the envelope.) Session id 0 is reserved for storage-layer events.
pub mod kind {
    /// Collector could not keep up / was blocked; body is UTF-8 "dropped=<n> first_us=<t> last_us=<t>".
    pub const SYSTEM_GAP: u16 = 1;
    /// Previous run did not shut down cleanly; body is UTF-8 "unclean_shutdown last_durable_seq=<n>".
    pub const SYSTEM_RECOVERED: u16 = 2;
}
