# Key management procedures

## Model
One X25519 key pair (used through HPKE, docs/HPKE.md) and one Ed25519 signing key per *key set*. The collector holds only the public key (stored in the
database `keys` table). The private key is split with Shamir secret sharing (2 of 2):

* **Share A** – `<data-dir>\keys\<key-id>.sharea`, wrapped with Windows DPAPI (current user).
* **Share B** – a text file you keep on a normal USB drive (`HERMES-SHARE-B v2`, hex). It also carries the
  Ed25519 *verifying* (public) key that the viewer trusts for record signatures.
* **Signing key** – `<data-dir>\keys\<key-id>.signkey`, DPAPI-protected, read only by the collector. It is
  deliberately *not* in the Share-A backup and not on the USB drive.

Either share alone is statistically independent of the key. The key exists in memory only
while the viewer is open, and is checked against its fingerprint (key id) after every
reconstruction, because Shamir reconstruction from a wrong share silently yields garbage.

## 1. Generate (first run)
Normally: `hermes init --usb E:\` (same steps, simpler). The `hermes-keytool` commands below are for rotation, backup, restore and checks.

`hermes-keytool init --data-dir D --share-b E:\hermes-share-b.key`
The tool generates the key pair and signing key, writes Share A, the signing key and Share B, **reads them
back and reconstructs the key**, and only then registers the public keys in the database. If the USB write fails, Share A is removed again.
It refuses to overwrite an existing Share B file.

## 2. Back up (do this immediately)
1. Copy `hermes-share-b.key` to a **second** USB drive/location (it is a small text file; a
   printout also works).
2. `hermes-keytool backup-share-a ... --out share-a.backup` (passphrase >= 12 chars,
   Argon2id 64 MiB / 3 passes). Store this file **away from both USB copies**.
3. Run `verify` against each Share B copy.
Never keep Share B and the Share A backup together: together with the passphrase they are the key.

## 3. Daily use
Collector: needs nothing but the public key; USB may stay out. Viewer: asks for the Share B
file, reads Share A through DPAPI, reconstructs in memory, keeps the session alive if the USB
is removed, and zeroizes the key on exit.

## 4. Rotation
`hermes-keytool rotate --data-dir D --share-b E:\hermes-share-b-2.key` (use a **new** file).
New records use the new key (a running collector calls `reload_active_key`). Old records stay
under the old key: keep the old Share A/Share B forever, the viewer needs one share pair per
key id it displays. Rotate when a USB drive was lost/copied, annually, or after suspected
exposure. Rotation does **not** re-encrypt history (not implemented): if an old key set is
compromised, everything encrypted under it is exposed.

## 5. Recovery
| Situation | Result / action |
|---|---|
| PC dead, new Windows profile, have Share A backup + passphrase + Share B | `restore-share-a --backup file`, then open viewer with Share B. History fully readable. |
| Lost USB, have second copy of Share B | Use the copy. Consider `rotate`. |
| Lost **Share B everywhere** (Share A intact) | History under that key is **permanently unrecoverable**. Nothing can derive B from A. New data keeps working after `rotate`. |
| Lost **Share A** (profile reset/reinstall/new Windows account) and no backup | Same: permanently unrecoverable. DPAPI blobs do not survive profile loss. |
| Lost the **signing key** (`.signkey`: profile reset) | Old history stays readable and its signatures stay verifiable (verifying key is on the USB). The collector can no longer sign under that key set: run `rotate`. |
| Forgot backup passphrase | Backup is useless; same as lost A if no other copy. |
| Lost A **and** B | Permanently unrecoverable. |
| Database corrupted | Collector quarantines `hermes.db.corrupt-<ms>`, salvages readable rows, continues. Lost ranges appear as sequence gaps. |

There is no key escrow and no backdoor by design.

## 6. Upgrading from the previous milestone
See docs/MIGRATION.md: existing key sets keep working for reading; the collector requires a signed key set, so
run `rotate` once.
