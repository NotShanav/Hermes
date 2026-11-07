# Record encryption: HPKE (RFC 9180)

Replaces the previous hand-composed ECDH + HKDF + XChaCha20-Poly1305 construction (kept only as a read-only
legacy decryptor, `hermes_crypto::legacy`, for old archives).

## Library
`hpke` **0.15.1** (https://github.com/rozbb/rust-hpke), default features off; enabled: `x25519`, `chacha`,
`alloc`, `getrandom`. crates.io shows a release in Oct 2026; MSRV 1.87. It is built on the same RustCrypto
primitives already in the tree (`x25519-dalek` 3, `chacha20poly1305` 0.11, `hkdf` 0.13, `sha2` 0.11), so no
parallel crypto stack was added.

Maintenance / review status (honest): the crate implements RFC 9180 and is tested against the official RFC test
vectors; its README states it has **not** had a paid third-party audit (earlier version 0.8 had an internal
review at Cloudflare). We run one RFC 9180 test vector (Appendix A.2.1) in our own test-suite
(`rfc9180_known_answer_for_our_cipher_suite`) to pin the exact cipher-suite constants. An independent review of
Hermes before trusting irreplaceable data is still recommended.

## Mode and algorithms (fixed; no negotiation, no agility)
| Item | Value |
|---|---|
| HPKE mode | `0x00` **Base** |
| KEM | `0x0020` DHKEM(X25519, HKDF-SHA256) |
| KDF | `0x0001` HKDF-SHA256 |
| AEAD | `0x0003` ChaCha20-Poly1305 |
| API | single-shot `single_shot_seal` / `single_shot_open` |
| `info` | `"hermes/record/v2 info" ‖ key_id` |
| `aad` | `"hermes/record/v2 aad" ‖ key_id ‖ seq(u64 LE) ‖ ts_us(i64 LE)` |

Wire format v2: `0x02 ‖ enc(32) ‖ ciphertext ‖ tag(16)`. The plaintext is the padded record envelope
(kind, session id, body; see docs/METADATA.md).

### Why ChaCha20-Poly1305 and not XChaCha20-Poly1305
RFC 9180 / the IANA HPKE registry define no XChaCha20 AEAD, and you required a vetted construction without custom
parts. The usual reason for XChaCha (random 96-bit nonce collisions) does not apply: every record is its own HPKE
message with a fresh ephemeral key, so each record has a fresh AEAD key *and* nonce from the key schedule.

### Why Base mode (not Auth / AuthPSK)
Auth mode would authenticate the sender to the recipient with a *static sender key held by the collector*. That is
exactly the key a signature scheme needs, but HPKE-Auth ties authenticity to confidentiality, only convinces the
holder of the private key (the viewer, after unlocking), and would make each record's authenticity unverifiable
by third parties or by us in the database. We keep encryption (Base) and authenticity (Ed25519 hash-chain
checkpoints, docs/SECURITY.md) separate and independently testable.

## Key compatibility / two-share requirement (unchanged)
HPKE's X25519 private key is the same 32-byte scalar the previous milestone used, so **existing key sets, Share A,
Share B and the Shamir split are untouched** (test `existing_x25519_key_sets_are_hpke_compatible` checks 20 key
sets derive identical public keys through HPKE). The private key is still split with Shamir 2-of-2; both shares
are required to reconstruct it; the viewer verifies the reconstructed key against the key id; the collector uses
only the public key and never touches a share.

## Security assumptions
* DHKEM(X25519) is IND-CCA2 secure under standard Diffie-Hellman assumptions in the random-oracle model
  (the HPKE analysis by Alwen, Blanchet, Hauck, Kiltz, Lipp, Riepel); HKDF-SHA256 behaves as an extractor/PRF;
  ChaCha20-Poly1305 is a secure AEAD; the OS RNG (`getrandom`) is sound.
* The viewer machine that reconstructs the key is not compromised while it is open.
* Compromise of both shares of a key set exposes every record sealed to it (no forward secrecy by design: the
  collector cannot hold a ratcheting secret without being able to decrypt).
* HPKE provides no sender authenticity in Base mode: see docs/SECURITY.md for the signature layer.
