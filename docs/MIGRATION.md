# Format changes in this release and what they mean for existing data

Applies to anyone who ran the previous drop (schema v1, record format v1). Nothing here is applied silently:
the database backs itself up first and every step is explicit.

| Layer | Before | Now | Effect on existing data |
|---|---|---|---|
| Record encryption | X25519+HKDF+XChaCha20 (custom composition), record format v1 | HPKE (RFC 9180), record format v2 | Old rows stay v1 and remain readable (`fmt = 1`). New rows are v2. |
| Plaintext metadata | `session_id`, `kind` columns; sessions table | encrypted in the record envelope; padding | Old rows keep their plaintext `legacy_session_id/legacy_kind`; old `sessions` table is renamed `legacy_sessions`. They stay plaintext-labelled **until re-encrypted** (tool not written yet). |
| Database schema | `user_version` 1 | `user_version` 2 | Automatic on first open; `hermes.db.pre-migration-v1.bak` is written first (VACUUM INTO); the migration is one transaction and rolls back as a whole on failure. Older builds refuse to open a v2 database (no silent downgrade). |
| Authenticity | none | Ed25519-signed hash-chain checkpoints | v1 rows can never be covered by a signature (reported as `legacy_unsigned_rows`). |
| USB key file | `HERMES-SHARE-B v1` | `v2` adds `sign_pub` | v1 files still load; they give no signature trust anchor. |
| Local key files | `<id>.sharea` | adds `<id>.signkey` (DPAPI) | created by `init`/`rotate` only |

## Required action for existing key sets
The new collector **refuses to run** on a key set that has no signing key (`KeySetNotSigned`), instead of silently
writing unsigned data. Run `hermes-keytool rotate --data-dir D --share-b NEW-USB-FILE`. Old history stays readable
with the old Share A/B; new history uses the new key set. Keep the old USB file forever.

## What is NOT migrated (honest)
* Legacy rows are not re-encrypted to v2, so their session ids/kinds remain visible as plaintext columns and
  their sizes are not padded. A `reencrypt` command (needs both shares of the old key set) is the follow-up; say
  if you want it next.
* There are no real user archives from before this release as far as I know; this path is covered by a
  fixture-based test (`v1_database_migrates_keeps_legacy_rows_readable...`) that builds a genuine v1 database with
  real v1 ciphertext. It has not been run against data from your machine.

## Old tests
All previous cryptographic and database tests were kept and adapted to the new APIs (36 crypto tests, 25 DB
tests now vs 24 + 20 before). Behavioural changes in tests: key-id mismatch is now an authentication failure
rather than a labelled error (the key id moved out of the payload); sessions are no longer database rows.
