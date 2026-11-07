# What is visible without the keys (metadata policy)

Decision (yours): timestamps and sequence numbers stay readable for chronological sorting; session ids,
application names, window titles, event kinds, event contents and all other identifying activity
information are encrypted; everything else is kept to the minimum below.

## The database file `hermes.db` (anyone who can read the file, no keys at all)

| Where | Visible | Why it is needed |
|---|---|---|
| `events.seq` | gap-free event number | ordering, deletion detection |
| `events.ts_us` | wall-clock time, microseconds | chronological browsing/sorting without decrypting |
| `events.key_id` | 128-bit fingerprint of the public key the record is sealed to | the viewer must know which share pair to ask for |
| `events.fmt` | `2` (current) or `1` (migrated legacy row) | format dispatch |
| `events.payload` **length** | `1 + 32 + 16 + padded` bytes, padded to a multiple of 256 | unavoidable; padding hides the exact size (a title of 10 or 200 characters is indistinguishable) |
| `checkpoints` | seq ranges, hash-chain values, Ed25519 signatures, signer fingerprint | authenticity (docs/SECURITY.md) |
| `keys` | public keys, signing verifying keys, created/retired times, active flag | collector encrypts with the active public key |
| `meta` | clean-shutdown flag, last integrity-check time, `min_next_seq` | crash recovery |
| `legacy_sessions`, `events.legacy_session_id/legacy_kind` | **only for rows written before the migration**: session ids, kinds, session start/end times | kept so old archives stay readable; see docs/MIGRATION.md |

**Encrypted (inside each record, not visible):** event kind, session id, application name, executable path,
process id, window handle, window class, window title, title/identity status, focus durations, the monotonic
clock value, session start/end reasons, "excluded application" markers, gap markers and recovery markers.

### What can still be inferred from the visible columns (be aware of this)
* **When** you were using the computer and **how busy** you were: timestamps and the number/size-buckets of
  records per time period. That is inherent in keeping timestamps visible. Each flush (about every 2 s of
  activity) also adds one checkpoint row.
* Gaps in time = machine off, locked, asleep or collection paused (the *reason* is encrypted).
* Roughly how large events are (256-byte buckets), e.g. a burst of larger records.
* The sizes of the `-wal` file and database, file-system timestamps, backups the OS makes.

## With only one share
* **Only Share A** (the DPAPI file on the PC): nothing more. Share A alone is independent of the private key,
  and it is DPAPI-encrypted for your Windows account in addition.
* **Only Share B** (the USB file): nothing more about the archive. It contains Share B (independent of the key),
  the key id and the *public* signature-verification key.
* **Share A + Share B**: everything, including all plaintext metadata above.

## The collector process
While running it necessarily handles window titles etc. in memory in plaintext (it must read them to record
them); it never stores them unencrypted, never holds the decryption key, and cannot decrypt what it wrote.
The console build prints `kind` and `session id` of every event to stderr unless `--quiet` is given: use
`--quiet` outside testing.

## What changed from the previous milestone
`session_id` and `kind` columns are gone for new rows; payload padding was added; the key id moved out of the
payload into the `key_id` column (it is bound into the HPKE info/AAD instead).
