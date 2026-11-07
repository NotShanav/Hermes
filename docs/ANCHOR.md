# Rollback / truncation protection: the USB checkpoint anchor

**Status: design proposal only. Nothing in this document is implemented in Beta 0.1.** Until it is built, deletion of the newest records and restoring an older archive copy are NOT detected.

## The remaining limitation (as shipped in drop 2)
The signed hash chain (docs/SECURITY.md) proves that every row *present* in the archive is genuine, in order, and
unmodified. It cannot prove that rows are *absent for the right reason*:

* **Truncation** — an attacker deletes the newest rows *and* their checkpoints. What remains is a perfectly valid, shorter archive.
* **Rollback** — an attacker restores an older copy of `hermes.db` over the current one.
* **Head truncation** — deleting the oldest rows+checkpoints leaves a valid suffix (first checkpoint's `prev_chain` is not pinned).

A signature only attests "I signed this"; nothing in the database can attest "this is all of it", because whoever
can delete rows can delete the evidence of the deletion. The test `documented_limitation_truncation_to_an_older_checkpoint_is_not_detectable`
pins this behaviour. A *trusted external witness* is required; you ruled out network witnesses, so the witness
is the thing you already carry: **the USB drive**.

## Proposal: signed checkpoint anchors on the USB drive (offline, no new secrets)
An **anchor** is simply a copy of one signed checkpoint (first/last seq, previous/last chain value, signer id,
Ed25519 signature) appended to a small text file on the USB drive: `hermes-anchors-<key-id>.log`.

*Why this works.* The copy is self-authenticating (signature verifies with the verifying key in Share B), it contains
the chain head at that moment, and it lives where the attacker who only controls the PC cannot reach. If the
archive later shows fewer rows than an anchor records, or a different chain head for the same sequence number, the
archive was truncated, rolled back or forked.

### Procedure (PROPOSED: **not implemented in Beta 0.1**)
1. User plugs in the USB drive and opens the verification tool/Viewer (both shares present).
2. The tool verifies the whole archive (chain + signatures) with the key from the USB. If **not clean**, it refuses to update the anchor.
3. **Check:** compare against every valid anchor in the file. For the newest valid anchor A:
   * archive contains a checkpoint with `last_seq == A.last_seq` and the same chain value → `OK`;
   * archive's newest verified checkpoint is older than A → **`TRUNCATED/ROLLED BACK`** (reports how many events are missing);
   * same `last_seq`, different chain/signature → **`FORKED/REPLACED`**;
   * anchor signature invalid → anchor ignored and reported as `TAMPERED ANCHOR` (never trusted).
4. **Advance:** if clean and the archive's newest checkpoint is newer than the last anchor, append it as a new anchor
   (append-only file; the last 32 anchors are kept in view, the file is never rewritten in place by the tool).

### What it detects, what it does not
| Attack | Detected |
|---|---|
| Truncate/roll back the PC archive to before the last anchor | **Yes** (at the next USB verification) |
| Replace the archive with a forged one | Yes (needs the signing key, and its chain must still contain the anchored checkpoint) |
| Delete or alter the anchor file *without* the signing key | Alteration: yes (signature). Deletion of entries: **no**, it only removes protection (keep a second USB copy; the tool warns when the file is missing/empty although the archive exists) |
| Truncate **after** the last anchor | **No**: only what happened since the last anchor is unprotected. The exposure window is "time since the last time you verified with the USB". |
| Attacker who holds the PC *and* the USB | Can truncate and rewrite anchors: out of scope (they have both shares anyway) |
| Malware as your user with the signing key | Can forge new history after the last anchor but cannot rewrite anchored history |

### Operational guidance
Anchor frequency is the security parameter. Verifying weekly leaves a one-week blind window. Keep **two USB drives**
(both carry Share B; update both) so loss/theft of one does not erase the witness.

### Not implemented yet (proposed phase 2: collector-side opportunistic anchoring)
When the USB drive is inserted, the collector could append the *current* checkpoint to the anchors file
automatically (device-arrival message, event-driven: no polling), shrinking the exposure window to the time since
the stick was last plugged in, without opening the Viewer. It needs only the anchors file path (found by its
file name on a newly arrived removable volume), **not** Share B, so the collector never reads a share. It is
deliberately left out of milestone 5: it writes to removable media from the always-on process, which deserves its own
review. The tray shows no anchor status for the same reason (the collector cannot know the USB state).
