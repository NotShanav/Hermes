//! Cryptographic test-suite (platform independent; uses TestProtector in place of DPAPI).
use crate::error::CryptoError;
use crate::keys::*;
use crate::legacy::{LegacyContext, LegacySealer};
use crate::protector::TestProtector;
use crate::provision::*;
use crate::record;
use crate::seal::*;
use crate::shares::*;
use crate::sign::*;

/// Temp dir; honours HERMES_TEST_DIR so the suite can run on NTFS / exFAT / FAT32 volumes.
fn tempdir() -> std::io::Result<tempfile::TempDir> {
    match std::env::var_os("HERMES_TEST_DIR") {
        Some(d) => {
            std::fs::create_dir_all(&d)?;
            tempfile::Builder::new()
                .prefix("hermes-test-")
                .tempdir_in(d)
        }
        None => tempfile::tempdir(),
    }
}

fn ctx() -> RecordContext {
    RecordContext {
        seq: 42,
        ts_us: 1_700_000_000_123_456,
    }
}

fn setup() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    TestProtector,
    PublicKeyInfo,
    std::path::PathBuf,
) {
    let local = tempdir().unwrap();
    let usb = tempdir().unwrap();
    let prot = TestProtector::new();
    let b = usb.path().join("hermes-share-b.key");
    let info = create_key_set(local.path(), &b, &prot).unwrap().info;
    (local, usb, prot, info, b)
}

#[test]
fn roundtrip_with_both_shares() {
    let (local, _usb, prot, info, b) = setup();
    let sealer = Sealer::new(info).unwrap();
    let sealed = sealer.seal(&ctx(), b"hello hermes").unwrap();
    let key = load_private_key(local.path(), &info.key_id, &b, &prot).unwrap();
    let opener = Opener::new(key);
    assert_eq!(&**opener.open(&ctx(), &sealed).unwrap(), b"hello hermes");
}

#[test]
fn empty_and_large_plaintexts() {
    let (local, _u, prot, info, b) = setup();
    let s = Sealer::new(info).unwrap();
    let o = Opener::new(load_private_key(local.path(), &info.key_id, &b, &prot).unwrap());
    for n in [0usize, 1, 31, 32, 4096, 1_000_000] {
        let pt = vec![0xA5u8; n];
        let ct = s.seal(&ctx(), &pt).unwrap();
        assert_eq!(&**o.open(&ctx(), &ct).unwrap(), &pt[..]);
    }
    let too_big = vec![0u8; MAX_PLAINTEXT + 1];
    assert!(matches!(
        s.seal(&ctx(), &too_big),
        Err(CryptoError::RecordTooLarge)
    ));
}

#[test]
fn sealing_is_randomised() {
    let (_l, _u, _p, info, _b) = setup();
    let s = Sealer::new(info).unwrap();
    let a = s.seal(&ctx(), b"same").unwrap();
    let b = s.seal(&ctx(), b"same").unwrap();
    assert_ne!(a, b);
    assert_ne!(
        &a[1..33],
        &b[1..33],
        "HPKE encapsulated (ephemeral) keys must differ per record"
    );
}

#[test]
fn every_single_byte_flip_is_rejected() {
    let (local, _u, prot, info, b) = setup();
    let s = Sealer::new(info).unwrap();
    let o = Opener::new(load_private_key(local.path(), &info.key_id, &b, &prot).unwrap());
    let sealed = s.seal(&ctx(), b"tamper evident payload").unwrap();
    for i in 0..sealed.len() {
        let mut t = sealed.clone();
        t[i] ^= 0x01;
        assert!(o.open(&ctx(), &t).is_err(), "flip at byte {i} was accepted");
    }
}

#[test]
fn truncation_and_extension_rejected() {
    let (local, _u, prot, info, b) = setup();
    let s = Sealer::new(info).unwrap();
    let o = Opener::new(load_private_key(local.path(), &info.key_id, &b, &prot).unwrap());
    let sealed = s.seal(&ctx(), b"abcdef").unwrap();
    for n in 0..sealed.len() {
        assert!(
            o.open(&ctx(), &sealed[..n]).is_err(),
            "truncation to {n} accepted"
        );
    }
    let mut ext = sealed.clone();
    ext.push(0);
    assert!(o.open(&ctx(), &ext).is_err());
}

#[test]
fn record_is_bound_to_its_row_context_and_key() {
    let (local, _u, prot, info, b) = setup();
    let s = Sealer::new(info).unwrap();
    let o = Opener::new(load_private_key(local.path(), &info.key_id, &b, &prot).unwrap());
    let sealed = s.seal(&ctx(), b"bound").unwrap();
    let c = ctx();
    for bad in [
        RecordContext {
            seq: c.seq + 1,
            ..c
        },
        RecordContext {
            ts_us: c.ts_us + 1,
            ..c
        },
    ] {
        assert!(matches!(
            o.open(&bad, &sealed),
            Err(CryptoError::AuthenticationFailed)
        ));
    }
    // A record presented under another key's identity (changed key_id column) fails too.
    let (l2, _u2, p2, info2, b2) = setup();
    let o2 = Opener::new(load_private_key(l2.path(), &info2.key_id, &b2, &p2).unwrap());
    assert!(matches!(
        o2.open(&c, &sealed),
        Err(CryptoError::AuthenticationFailed)
    ));
}

#[test]
fn single_share_alone_cannot_reconstruct() {
    let ks = generate_key_set().unwrap();
    // Same share twice (attacker has only one share):
    let dup = SecretShare::from_bytes(ks.share_a.as_bytes().to_vec()).unwrap();
    assert!(matches!(
        reconstruct_private_key(&ks.share_a, &dup, &ks.info.key_id),
        Err(CryptoError::DuplicateShares)
    ));
    // One real share + attacker-chosen second share with the other index:
    let mut guess = vec![0u8; SHARE_LEN];
    guess[0] = ks.share_b.index();
    let guess = SecretShare::from_bytes(guess).unwrap();
    assert!(matches!(
        reconstruct_private_key(&ks.share_a, &guess, &ks.info.key_id),
        Err(CryptoError::WrongShares)
    ));
    // Share bytes are not the key and neither is a function the other can reveal trivially:
    assert_ne!(&ks.share_a.as_bytes()[1..], &ks.share_b.as_bytes()[1..]);
}

#[test]
fn shares_are_fresh_for_every_generation() {
    let a = generate_key_set().unwrap();
    let b = generate_key_set().unwrap();
    assert_ne!(a.info.key_id, b.info.key_id);
    assert_ne!(a.share_a.as_bytes(), b.share_a.as_bytes());
}

#[test]
fn wrong_share_from_another_key_is_detected() {
    let k1 = generate_key_set().unwrap();
    let k2 = generate_key_set().unwrap();
    // Correct A, but B from a different key set:
    assert!(matches!(
        reconstruct_private_key(&k1.share_a, &k2.share_b, &k1.info.key_id),
        Err(CryptoError::WrongShares)
    ));
    assert!(matches!(
        reconstruct_private_key(&k2.share_a, &k1.share_b, &k1.info.key_id),
        Err(CryptoError::WrongShares)
    ));
}

#[test]
fn corrupted_share_is_detected_not_silently_accepted() {
    let k = generate_key_set().unwrap();
    for i in 1..SHARE_LEN {
        let mut bad = k.share_b.as_bytes().to_vec();
        bad[i] ^= 0x80;
        let bad = SecretShare::from_bytes(bad).unwrap();
        assert!(matches!(
            reconstruct_private_key(&k.share_a, &bad, &k.info.key_id),
            Err(CryptoError::WrongShares)
        ));
    }
}

#[test]
fn missing_shares_fail_cleanly() {
    let (local, _usb, prot, info, b) = setup();
    // Missing USB file:
    std::fs::remove_file(&b).unwrap();
    match load_private_key(local.path(), &info.key_id, &b, &prot) {
        Err(CryptoError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected NotFound, got {other:?}"),
    }
    // Missing local share A (with a valid B):
    let (local2, _usb2, prot2, info2, b2) = setup();
    std::fs::remove_file(share_a_path(local2.path(), &info2.key_id)).unwrap();
    assert!(matches!(
        load_private_key(local2.path(), &info2.key_id, &b2, &prot2),
        Err(CryptoError::Io(_))
    ));
}

#[test]
fn share_b_file_corruption_and_mixups() {
    let (local, _usb, prot, info, b) = setup();
    let original = std::fs::read_to_string(&b).unwrap();

    // 1. Bit-rot: change one hex digit of the share -> checksum catches it.
    let mut rotted = original.clone();
    let pos = rotted.find("share: ").unwrap() + 8;
    let c = rotted.as_bytes()[pos];
    let repl = if c == b'0' { '1' } else { '0' };
    rotted.replace_range(pos..pos + 1, &repl.to_string());
    std::fs::write(&b, &rotted).unwrap();
    assert!(matches!(
        load_private_key(local.path(), &info.key_id, &b, &prot),
        Err(CryptoError::ChecksumMismatch)
    ));

    // 2. Deliberate edit with recomputed checksum -> reconstruction check catches it.
    let parsed = parse_share_b(&original).unwrap();
    let mut bytes = parsed.share.as_bytes().to_vec();
    bytes[5] ^= 0xFF;
    let forged = encode_share_b(
        &parsed.key_id,
        &SecretShare::from_bytes(bytes).unwrap(),
        &parsed.sign_public.unwrap(),
    );
    std::fs::write(&b, forged.as_bytes()).unwrap();
    assert!(matches!(
        load_private_key(local.path(), &info.key_id, &b, &prot),
        Err(CryptoError::WrongShares)
    ));

    // 3. Share file from a different key set (wrong USB stick).
    let other = generate_key_set().unwrap();
    write_share_b(&b, &other.info.key_id, &other.share_b, &[7u8; 32], true).unwrap();
    assert!(matches!(
        load_private_key(local.path(), &info.key_id, &b, &prot),
        Err(CryptoError::KeyIdMismatch { .. })
    ));

    // 4. Garbage / truncated / CRLF-converted file.
    std::fs::write(&b, "not a key").unwrap();
    assert!(load_private_key(local.path(), &info.key_id, &b, &prot).is_err());
    std::fs::write(&b, &original[..original.len() / 2]).unwrap();
    assert!(load_private_key(local.path(), &info.key_id, &b, &prot).is_err());
    std::fs::write(&b, original.replace('\n', "\r\n")).unwrap();
    load_private_key(local.path(), &info.key_id, &b, &prot)
        .expect("CRLF-converted file must still parse");
}

#[test]
fn open_session_survives_usb_removal_and_next_open_needs_usb_again() {
    let (local, _usb, prot, info, b) = setup();
    let sealed = Sealer::new(info).unwrap().seal(&ctx(), b"history").unwrap();
    let opener = Opener::new(load_private_key(local.path(), &info.key_id, &b, &prot).unwrap());
    std::fs::remove_file(&b).unwrap(); // USB pulled
    assert_eq!(&**opener.open(&ctx(), &sealed).unwrap(), b"history");
    drop(opener); // viewer closed: key zeroized on drop
    assert!(load_private_key(local.path(), &info.key_id, &b, &prot).is_err());
}

#[test]
fn collector_encrypts_with_public_key_only_and_usb_absent() {
    // The collector's whole view of the key system is `PublicKeyInfo`; no share files are read.
    let (local, usb, prot, info, b) = setup();
    let saved_usb_copy = std::fs::read(&b).unwrap();
    drop(usb); // USB directory deleted entirely: collector keeps working
    assert!(!b.exists());
    let sealed = Sealer::new(info)
        .unwrap()
        .seal(&ctx(), b"written while usb absent")
        .unwrap();

    // USB comes back later (same file restored): history written meanwhile is readable.
    let usb2 = tempdir().unwrap();
    let b2 = usb2.path().join("hermes-share-b.key");
    std::fs::write(&b2, saved_usb_copy).unwrap();
    let opener = Opener::new(load_private_key(local.path(), &info.key_id, &b2, &prot).unwrap());
    assert_eq!(
        &**opener.open(&ctx(), &sealed).unwrap(),
        b"written while usb absent"
    );
}

#[test]
fn share_a_is_bound_to_the_protector() {
    let (local, _usb, _prot, info, b) = setup();
    let other_machine = TestProtector::new();
    assert!(matches!(
        load_private_key(local.path(), &info.key_id, &b, &other_machine),
        Err(CryptoError::Protector(_))
    ));
}

#[test]
fn share_a_file_tampering_detected() {
    let (local, _usb, prot, info, b) = setup();
    let p = share_a_path(local.path(), &info.key_id);
    let orig = std::fs::read(&p).unwrap();
    for i in 0..orig.len() {
        let mut t = orig.clone();
        t[i] ^= 1;
        std::fs::write(&p, &t).unwrap();
        assert!(
            load_private_key(local.path(), &info.key_id, &b, &prot).is_err(),
            "byte {i}"
        );
    }
}

#[test]
fn backup_and_disaster_recovery_on_new_machine() {
    let (local, usb, prot, info, b) = setup();
    let sealed = Sealer::new(info)
        .unwrap()
        .seal(&ctx(), b"old history")
        .unwrap();

    let pass = "correct horse battery staple";
    let backup = backup_share_a(
        local.path(),
        &info.key_id,
        pass,
        KdfParams::TEST_ONLY,
        &prot,
    )
    .unwrap();
    assert!(!backup.contains(&info.key_id.to_hex().repeat(2)));

    // Old PC is gone: new empty profile, new protector (new DPAPI master key).
    drop(local);
    let new_local = tempdir().unwrap();
    let new_prot = TestProtector::new();

    // wrong passphrase
    assert!(matches!(
        restore_share_a(
            new_local.path(),
            &backup,
            "wrong passphrase!!",
            &new_prot,
            false
        ),
        Err(CryptoError::AuthenticationFailed)
    ));
    // too-weak passphrase on export
    let ks = generate_key_set().unwrap();
    assert!(matches!(
        export_share_a_backup(&ks.info.key_id, &ks.share_a, "short", KdfParams::TEST_ONLY),
        Err(CryptoError::WeakPassphrase(_))
    ));
    // correct passphrase
    let id = restore_share_a(new_local.path(), &backup, pass, &new_prot, false).unwrap();
    assert_eq!(id, info.key_id);
    let opener = Opener::new(load_private_key(new_local.path(), &id, &b, &new_prot).unwrap());
    assert_eq!(&**opener.open(&ctx(), &sealed).unwrap(), b"old history");
    drop(usb);
}

#[test]
fn backup_alone_or_usb_alone_is_not_enough() {
    let (local, _usb, prot, info, _b) = setup();
    let backup = backup_share_a(
        local.path(),
        &info.key_id,
        "a long passphrase 1",
        KdfParams::TEST_ONLY,
        &prot,
    )
    .unwrap();
    // Backup holds only share A (index differs from B's): using it twice cannot reconstruct.
    let (_, a) = import_share_a_backup(&backup, "a long passphrase 1").unwrap();
    let a2 = SecretShare::from_bytes(a.as_bytes().to_vec()).unwrap();
    assert!(matches!(
        reconstruct_private_key(&a, &a2, &info.key_id),
        Err(CryptoError::DuplicateShares)
    ));
}

#[test]
fn backup_file_tampering_and_hostile_params() {
    let ks = generate_key_set().unwrap();
    let pass = "a long passphrase 2";
    let good =
        export_share_a_backup(&ks.info.key_id, &ks.share_a, pass, KdfParams::TEST_ONLY).unwrap();
    assert!(import_share_a_backup(&good, pass).is_ok());

    // change KDF params in the file (also covered by AAD) -> must not decrypt
    let t = good.replace("t=1", "t=2");
    assert!(import_share_a_backup(&t, pass).is_err());
    // swap key id
    let other = generate_key_set().unwrap();
    let t = good.replace(&ks.info.key_id.to_hex(), &other.info.key_id.to_hex());
    assert!(import_share_a_backup(&t, pass).is_err());
    // flip data byte
    let idx = good.rfind("data: ").unwrap() + 6;
    let mut bytes = good.clone().into_bytes();
    bytes[idx] = if bytes[idx] == b'0' { b'1' } else { b'0' };
    assert!(import_share_a_backup(&String::from_utf8(bytes).unwrap(), pass).is_err());
    // memory-bomb parameters are rejected before any allocation
    let t = good.replace("m=8", "m=4000000000");
    assert!(matches!(
        import_share_a_backup(&t, pass),
        Err(CryptoError::BadKdfParams)
    ));
}

#[test]
fn rotation_keeps_history_readable_and_isolates_keys() {
    let local = tempdir().unwrap();
    let usb = tempdir().unwrap();
    let prot = TestProtector::new();
    let b1 = usb.path().join("share-b-1.key");
    let b2 = usb.path().join("share-b-2.key");
    let k1 = create_key_set(local.path(), &b1, &prot).unwrap().info;
    let rec1 = Sealer::new(k1)
        .unwrap()
        .seal(&ctx(), b"before rotation")
        .unwrap();

    let k2 = create_key_set(local.path(), &b2, &prot).unwrap().info; // rotation
    assert_ne!(k1.key_id, k2.key_id);
    let rec2 = Sealer::new(k2)
        .unwrap()
        .seal(&ctx(), b"after rotation")
        .unwrap();

    let o1 = Opener::new(load_private_key(local.path(), &k1.key_id, &b1, &prot).unwrap());
    let o2 = Opener::new(load_private_key(local.path(), &k2.key_id, &b2, &prot).unwrap());
    assert_eq!(&**o1.open(&ctx(), &rec1).unwrap(), b"before rotation");
    assert_eq!(&**o2.open(&ctx(), &rec2).unwrap(), b"after rotation");
    // New key cannot read old data and vice-versa (compromise of the new key set != old data).
    assert!(matches!(
        o2.open(&ctx(), &rec1),
        Err(CryptoError::AuthenticationFailed)
    ));
    assert!(matches!(
        o1.open(&ctx(), &rec2),
        Err(CryptoError::AuthenticationFailed)
    ));
    // Shares from different generations don't mix.
    let (_, a1) = (
        0,
        read_share_a(&share_a_path(local.path(), &k1.key_id), &k1.key_id, &prot).unwrap(),
    );
    let sb2 = read_share_b(&b2, &k2.key_id).unwrap();
    assert!(matches!(
        reconstruct_private_key(&a1, &sb2, &k1.key_id),
        Err(CryptoError::WrongShares)
    ));
}

#[test]
fn create_key_set_never_overwrites_and_rolls_back_on_failure() {
    let local = tempdir().unwrap();
    let usb = tempdir().unwrap();
    let prot = TestProtector::new();
    let b = usb.path().join("share-b.key");
    std::fs::write(&b, "precious existing file").unwrap();
    assert!(matches!(
        create_key_set(local.path(), &b, &prot),
        Err(CryptoError::WouldOverwrite(_))
    ));
    assert_eq!(
        std::fs::read_to_string(&b).unwrap(),
        "precious existing file"
    );
    assert_eq!(
        std::fs::read_dir(local.path())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );

    // USB write failure (parent "directory" is actually a file): share A must be rolled back.
    let bad = b.join("nested.key");
    assert!(create_key_set(local.path(), &bad, &prot).is_err());
    let leftovers: Vec<_> = std::fs::read_dir(local.path()).unwrap().collect();
    assert!(
        leftovers.is_empty(),
        "orphaned share A left behind: {leftovers:?}"
    );
}

#[test]
fn sealer_rejects_inconsistent_public_key_info() {
    let ks = generate_key_set().unwrap();
    let mut forged = ks.info;
    forged.public[0] ^= 1; // attacker swaps public key in a database row but keeps the key id
    assert!(Sealer::new(forged).is_err());
    // low-order public key (all zero) with a self-consistent id is refused (at construction or seal time)
    let zero = PublicKeyInfo::from_public([0u8; 32]);
    match Sealer::new(zero) {
        Err(_) => {}
        Ok(s) => assert!(matches!(
            s.seal(&ctx(), b"x"),
            Err(CryptoError::InvalidPublicKey)
        )),
    }
}

#[test]
fn secrets_are_redacted_in_debug_output() {
    let ks = generate_key_set().unwrap();
    let shown = format!("{:?} {:?}", ks.share_a, ks.share_b);
    let hex_a = crate::hexutil::encode(&ks.share_a.as_bytes()[1..]);
    assert!(!shown.contains(&hex_a));
    let (local, _u, prot, info, b) = setup();
    let key = load_private_key(local.path(), &info.key_id, &b, &prot).unwrap();
    assert!(format!("{key:?}").contains("redacted"));
}

#[test]
fn share_file_parsers_reject_malformed_input() {
    for bad in [
        "",
        "HERMES-SHARE-B v1",
        "HERMES-SHARE-B v2\nkey_id: 00\nshare: 00\ncheck: 00\n",
    ] {
        assert!(parse_share_b(bad).is_err());
    }
    let ks = generate_key_set().unwrap();
    let ok = encode_share_b(&ks.info.key_id, &ks.share_b, &[9u8; 32]);
    assert!(parse_share_b(&ok).is_ok());
    // uppercase hex (non-canonical) is refused
    assert!(parse_share_b(&ok.to_uppercase()).is_err());
    // trailing junk
    assert!(parse_share_b(&format!("{}extra: 1\n", &*ok)).is_err());
}

// ====================================================================== HPKE specifics

fn hx(s: &str) -> Vec<u8> {
    crate::hexutil::decode(s).unwrap()
}

#[test]
fn rfc9180_known_answer_for_our_cipher_suite() {
    // RFC 9180 Appendix A.2.1 (DHKEM(X25519,HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305, mode Base),
    // first encryption. Proves the suite our Sealer/Opener use is the standardised one.
    use hpke::{
        aead::ChaCha20Poly1305, kdf::HkdfSha256, kem::X25519HkdfSha256, Deserializable, Kem,
        OpModeR, Serializable,
    };
    type K = X25519HkdfSha256;
    let sk = <K as Kem>::PrivateKey::from_bytes(&hx(
        "8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb",
    ))
    .unwrap();
    let pk = <K as Kem>::sk_to_pk(&sk);
    assert_eq!(
        pk.to_bytes().as_slice(),
        &hx("4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a")[..]
    );
    let enc = <K as Kem>::EncappedKey::from_bytes(&hx(
        "1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a",
    ))
    .unwrap();
    let pt = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, K>(
        &OpModeR::Base,
        &sk,
        &enc,
        &hx("4f6465206f6e2061204772656369616e2055726e"),
        &hx("1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db21993c62ce81883d2dd1b51a28"),
        &hx("436f756e742d30"),
    )
    .unwrap();
    assert_eq!(
        pt,
        hx("4265617574792069732074727574682c20747275746820626561757479")
    );
}

#[test]
fn existing_x25519_key_sets_are_hpke_compatible() {
    // Key sets created by the previous milestone are plain X25519 key pairs; HPKE must derive the
    // same public key from the stored private scalar, so no key migration is needed.
    use hpke::{kem::X25519HkdfSha256, Deserializable, Kem, Serializable};
    for _ in 0..20 {
        let ks = generate_key_set().unwrap();
        let key = reconstruct_private_key(&ks.share_a, &ks.share_b, &ks.info.key_id).unwrap();
        let sk = <X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(&*key.to_bytes()).unwrap();
        let pk = <X25519HkdfSha256 as Kem>::sk_to_pk(&sk);
        assert_eq!(pk.to_bytes().as_slice(), &ks.info.public[..]);
    }
}

#[test]
fn wire_format_is_hpke_v2() {
    let ks = generate_key_set().unwrap();
    let sealed = Sealer::new(ks.info).unwrap().seal(&ctx(), b"abc").unwrap();
    assert_eq!(sealed[0], FORMAT_VERSION);
    assert_eq!(sealed.len(), 1 + 32 + 3 + 16, "version | enc | ct | tag");
}

#[test]
fn legacy_v1_records_remain_readable() {
    let ks = generate_key_set().unwrap();
    let lc = LegacyContext {
        seq: 5,
        ts_us: 99,
        session_id: 3,
        kind: 7,
    };
    let old = LegacySealer::new(ks.info)
        .unwrap()
        .seal(&lc, b"written by milestone 4")
        .unwrap();
    let o =
        Opener::new(reconstruct_private_key(&ks.share_a, &ks.share_b, &ks.info.key_id).unwrap());
    assert_eq!(
        &**o.open_legacy_v1(&lc, &old).unwrap(),
        b"written by milestone 4"
    );
    let bad = LegacyContext { kind: 8, ..lc };
    assert!(o.open_legacy_v1(&bad, &old).is_err());
    // and a v1 blob is not accepted by the v2 path
    assert!(o.open(&RecordContext { seq: 5, ts_us: 99 }, &old).is_err());
}

// ====================================================================== metadata envelope

#[test]
fn envelope_roundtrip_and_padding() {
    for n in [0usize, 1, 100, 240, 241, 255, 256, 600, 5000] {
        let body: Vec<u8> = (0..n).map(|i| i as u8).collect();
        let enc = record::encode(0x1234, 0xDEAD_BEEF_0000_0001, &body).unwrap();
        assert_eq!(enc.len() % record::PAD_BLOCK, 0);
        assert!(enc.len() >= n + record::HEADER_LEN);
        let dec = record::decode(&enc).unwrap();
        assert_eq!(
            (dec.kind, dec.session_id, &dec.body[..]),
            (0x1234, 0xDEAD_BEEF_0000_0001, &body[..])
        );
    }
}

#[test]
fn envelope_rejects_malformed() {
    let good = record::encode(1, 2, b"hello").unwrap().to_vec();
    let mut v = good.clone();
    v[0] = 9;
    assert!(matches!(
        record::decode(&v),
        Err(CryptoError::UnsupportedVersion(9))
    ));
    let mut v = good.clone();
    *v.last_mut().unwrap() = 1; // non-zero padding
    assert!(record::decode(&v).is_err());
    let mut v = good.clone();
    v[11..15].copy_from_slice(&(10_000u32).to_le_bytes()); // length beyond buffer
    assert!(record::decode(&v).is_err());
    assert!(record::decode(&good[..good.len() - 1]).is_err());
    assert!(record::decode(&[]).is_err());
}

#[test]
fn kind_session_and_body_are_inside_the_ciphertext_and_lengths_are_bucketed() {
    let (local, _u, prot, info, b) = setup();
    let s = Sealer::new(info).unwrap();
    let o = Opener::new(load_private_key(local.path(), &info.key_id, &b, &prot).unwrap());
    let marker = b"Quarterly-Salary-Review.xlsx - Excel";
    let sealed = s
        .seal_record(&ctx(), 0x4142, 0x1122_3344_5566_7788, marker)
        .unwrap();
    assert!(!sealed.windows(marker.len()).any(|w| w == marker));
    assert!(!sealed
        .windows(8)
        .any(|w| w == 0x1122_3344_5566_7788u64.to_le_bytes()));
    let r = o.open_record(&ctx(), &sealed).unwrap();
    assert_eq!(
        (r.kind, r.session_id, &r.body[..]),
        (0x4142, 0x1122_3344_5566_7788, &marker[..])
    );
    // Titles of different lengths within one 256-byte bucket produce identical blob sizes.
    let l1 = s.seal_record(&ctx(), 1, 1, &[b'a'; 10]).unwrap().len();
    let l2 = s.seal_record(&ctx(), 2, 9, &[b'b'; 200]).unwrap().len();
    let l3 = s.seal_record(&ctx(), 2, 9, &[b'b'; 300]).unwrap().len();
    assert_eq!(l1, l2);
    assert_eq!(l3, l1 + 256);
    // tampering with the envelope inside the AEAD is impossible without detection
    let mut t = sealed.clone();
    let n = t.len();
    t[n - 20] ^= 1;
    assert!(o.open_record(&ctx(), &t).is_err());
}

// ====================================================================== authenticity (signing)

#[test]
fn signed_checkpoints_verify_and_detect_any_field_change() {
    let (signer, _seed) = RecordSigner::generate().unwrap();
    let vk = signer.public();
    let body = CheckpointBody {
        signer_id: signer.signer_id(),
        first_seq: 10,
        last_seq: 20,
        prev_chain: [1; 32],
        last_chain: [2; 32],
    };
    let sig = signer.sign_checkpoint(&body);
    assert!(verify_checkpoint(&vk, &body, &sig).is_ok());
    for mutate in 0..5 {
        let mut b = body.clone();
        match mutate {
            0 => b.signer_id[0] ^= 1,
            1 => b.first_seq += 1,
            2 => b.last_seq += 1,
            3 => b.prev_chain[0] ^= 1,
            _ => b.last_chain[31] ^= 1,
        }
        assert!(
            verify_checkpoint(&vk, &b, &sig).is_err(),
            "mutation {mutate}"
        );
    }
    let mut bad = sig;
    bad[10] ^= 1;
    assert!(verify_checkpoint(&vk, &body, &bad).is_err());
    let (other, _) = RecordSigner::generate().unwrap();
    assert!(
        verify_checkpoint(&other.public(), &body, &sig).is_err(),
        "other signer"
    );
}

#[test]
fn chain_depends_on_every_input() {
    let kid = KeyId([3; 16]);
    let base = chain_next(&ZERO_CHAIN, 1, 5, &kid, b"p");
    assert_eq!(base, chain_next(&ZERO_CHAIN, 1, 5, &kid, b"p"));
    assert_ne!(base, chain_next(&[1; 32], 1, 5, &kid, b"p"));
    assert_ne!(base, chain_next(&ZERO_CHAIN, 2, 5, &kid, b"p"));
    assert_ne!(base, chain_next(&ZERO_CHAIN, 1, 6, &kid, b"p"));
    assert_ne!(base, chain_next(&ZERO_CHAIN, 1, 5, &KeyId([4; 16]), b"p"));
    assert_ne!(base, chain_next(&ZERO_CHAIN, 1, 5, &kid, b"q"));
}

#[test]
fn collector_loads_signer_without_any_decryption_share_and_viewer_gets_trust_anchor() {
    let local = tempdir().unwrap();
    let usb = tempdir().unwrap();
    let prot = TestProtector::new();
    let b = usb.path().join("b.key");
    let nk = create_key_set(local.path(), &b, &prot).unwrap();
    // collector side: only the DPAPI-protected signing seed is read
    std::fs::remove_file(share_a_path(local.path(), &nk.info.key_id)).unwrap();
    std::fs::remove_file(&b).unwrap();
    let signer = load_signer(local.path(), &nk.info.key_id, &prot).unwrap();
    assert_eq!(signer.public(), nk.sign_public);
    // signing seed bound to protector and tamper-evident
    assert!(load_signer(local.path(), &nk.info.key_id, &TestProtector::new()).is_err());
    let p = signing_key_path(local.path(), &nk.info.key_id);
    let orig = std::fs::read(&p).unwrap();
    for i in 0..orig.len() {
        let mut t = orig.clone();
        t[i] ^= 1;
        std::fs::write(&p, &t).unwrap();
        assert!(
            load_signer(local.path(), &nk.info.key_id, &prot).is_err(),
            "byte {i}"
        );
    }
}

#[test]
fn viewer_material_returns_signature_trust_anchor_from_usb() {
    let (local, _usb, prot, info, b) = setup();
    let (_k, vk) = load_viewer_material(local.path(), &info.key_id, &b, &prot).unwrap();
    assert!(vk.is_some());
    assert_eq!(
        vk.unwrap(),
        parse_share_b(&std::fs::read_to_string(&b).unwrap())
            .unwrap()
            .sign_public
            .unwrap()
    );
}

#[test]
fn v1_share_files_from_previous_milestone_still_load_without_signing_anchor() {
    let local = tempdir().unwrap();
    let usb = tempdir().unwrap();
    let prot = TestProtector::new();
    let ks = generate_key_set().unwrap();
    write_share_a(
        &share_a_path(local.path(), &ks.info.key_id),
        &ks.info.key_id,
        &ks.share_a,
        &prot,
        false,
    )
    .unwrap();
    let b = usb.path().join("old.key");
    std::fs::write(
        &b,
        encode_share_b_v1(&ks.info.key_id, &ks.share_b).as_bytes(),
    )
    .unwrap();
    let (key, vk) = load_viewer_material(local.path(), &ks.info.key_id, &b, &prot).unwrap();
    assert_eq!(key.info(), ks.info);
    assert!(vk.is_none(), "v1 key sets have no signing anchor");
    // and a v1 file with a tampered share is still caught
    let mut t = std::fs::read_to_string(&b).unwrap();
    let pos = t.find("share: ").unwrap() + 9;
    t.replace_range(
        pos..pos + 1,
        if &t[pos..pos + 1] == "0" { "1" } else { "0" },
    );
    std::fs::write(&b, t).unwrap();
    assert!(matches!(
        load_viewer_material(local.path(), &ks.info.key_id, &b, &prot),
        Err(CryptoError::ChecksumMismatch)
    ));
}
