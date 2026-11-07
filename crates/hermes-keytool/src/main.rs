//! hermes-keytool: key lifecycle management and filesystem validation.
//!
//!   init            --data-dir D --share-b F        create first key set (Share A + signing key -> D/keys, Share B -> F)
//!   rotate          --data-dir D --share-b F        create a new key set and make it the active key
//!   verify          --data-dir D --key-id ID --share-b F
//!   list            --data-dir D [--share-b F]
//!   backup-share-a  --data-dir D --key-id ID --out FILE      (prompts for passphrase)
//!   restore-share-a --data-dir D --backup FILE               (prompts for passphrase)
//!   fscheck         --dir PATH [--local-dir D] [--events N]  validate storage/key code on that volume
//!
//! Share A and the signing key are protected by Windows DPAPI. On non-Windows builds every command that
//! needs them fails with "no operating-system secret protector": there is no plaintext fallback.

mod fscheck;

use hermes_crypto::provision::{backup_share_a, create_key_set, restore_share_a, verify_key_set};
use hermes_crypto::shares::{peek_share_b_key_id, KdfParams};
use hermes_crypto::KeyId;
use hermes_db::{Database, OpenOptions};
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    cmd: String,
    data_dir: Option<PathBuf>,
    share_b: Option<PathBuf>,
    key_id: Option<String>,
    out: Option<PathBuf>,
    backup: Option<PathBuf>,
    dir: Option<PathBuf>,
    events: u32,
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or(
        "missing command (init|rotate|verify|list|backup-share-a|restore-share-a|fscheck)",
    )?;
    let mut a = Args {
        cmd,
        data_dir: None,
        share_b: None,
        key_id: None,
        out: None,
        backup: None,
        dir: None,
        events: 20_000,
    };
    while let Some(flag) = it.next() {
        let val = it.next().ok_or(format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--data-dir" | "--local-dir" => a.data_dir = Some(val.into()),
            "--share-b" => a.share_b = Some(val.into()),
            "--key-id" => a.key_id = Some(val),
            "--out" => a.out = Some(val.into()),
            "--backup" => a.backup = Some(val.into()),
            "--dir" => a.dir = Some(val.into()),
            "--events" => {
                a.events = val
                    .parse()
                    .map_err(|_| "--events must be a number".to_string())?
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(a)
}

fn need<T>(v: Option<T>, name: &str) -> Result<T, String> {
    v.ok_or(format!("{name} is required"))
}

fn key_id_arg(a: &Args) -> Result<KeyId, String> {
    let s = need(a.key_id.clone(), "--key-id")?;
    KeyId::from_hex(&s).ok_or("--key-id must be 32 hex characters".to_string())
}

fn run() -> Result<(), String> {
    let a = parse()?;
    let e = |x: &dyn std::fmt::Display| x.to_string();

    if a.cmd == "fscheck" {
        let dir = need(a.dir.clone(), "--dir")?;
        let local = a.data_dir.clone().unwrap_or_else(std::env::temp_dir);
        let prot = hermes_platform::default_protector().ok();
        let checks = fscheck::run(&dir, &local, prot.as_deref(), a.events);
        let mut failed = 0;
        for c in &checks {
            match &c.outcome {
                Ok(m) if c.skipped => println!("SKIP  {:<95} {m}", c.name),
                Ok(m) => println!("PASS  {:<95} {m} ({} ms)", c.name, c.ms),
                Err(m) => {
                    failed += 1;
                    println!("FAIL  {:<95} {m} ({} ms)", c.name, c.ms)
                }
            }
        }
        return if failed == 0 {
            Ok(())
        } else {
            Err(format!("{failed} check(s) failed"))
        };
    }

    let data = need(a.data_dir.clone(), "--data-dir")?;
    let keys_dir = data.join("keys");
    let db_path = data.join("hermes.db");

    match a.cmd.as_str() {
        "init" | "rotate" => {
            let share_b = need(a.share_b.clone(), "--share-b")?;
            let prot = hermes_platform::default_protector().map_err(|x| e(&x))?;
            let (mut db, _) =
                Database::open(&db_path, OpenOptions::default()).map_err(|x| e(&x))?;
            let has_active = db.active_key().map_err(|x| e(&x))?.is_some();
            if a.cmd == "init" && has_active {
                return Err("an active key already exists; use `rotate`".into());
            }
            if a.cmd == "rotate" && !has_active {
                return Err("no active key exists; use `init`".into());
            }
            // 1. create + read-back verify all key files  2. only then register the public keys.
            let nk = create_key_set(&keys_dir, &share_b, prot.as_ref()).map_err(|x| e(&x))?;
            db.register_key(&nk.info, Some(&nk.sign_public), true)
                .map_err(|x| e(&x))?;
            println!("key id: {}", nk.info.key_id);
            println!("Share A + signing key (DPAPI): {}", keys_dir.display());
            println!(
                "Share B: {}  <- keep on the USB drive; make a second copy",
                share_b.display()
            );
            println!("NEXT: run `backup-share-a` and store the backup file somewhere other than the USB drive.");
            println!("NOTE: restart a running collector so it picks up the new key set.");
            Ok(())
        }
        "verify" => {
            let id = key_id_arg(&a)?;
            let share_b = need(a.share_b.clone(), "--share-b")?;
            let prot = hermes_platform::default_protector().map_err(|x| e(&x))?;
            verify_key_set(&keys_dir, &id, &share_b, prot.as_ref()).map_err(|x| e(&x))?;
            println!("OK: Share A + this Share B reconstruct key {id}");
            Ok(())
        }
        "list" => {
            let (db, _) = Database::open(&db_path, OpenOptions::default()).map_err(|x| e(&x))?;
            for k in db.keys().map_err(|x| e(&x))? {
                let a_present =
                    hermes_crypto::shares::share_a_path(&keys_dir, &k.info.key_id).exists();
                let s_present =
                    hermes_crypto::shares::signing_key_path(&keys_dir, &k.info.key_id).exists();
                println!(
                    "{}  {}  share A here: {}  signing: {}",
                    k.info.key_id,
                    if k.active { "ACTIVE " } else { "retired" },
                    if a_present {
                        "yes"
                    } else {
                        "NO (restore from backup)"
                    },
                    match (k.sign_public.is_some(), s_present) {
                        (false, _) => "none (pre-signing key set; run `rotate`)",
                        (true, true) => "yes",
                        (true, false) =>
                            "key file MISSING on this PC (collector cannot sign; run `rotate`)",
                    }
                );
            }
            if let Some(p) = a.share_b {
                if let Ok(id) = peek_share_b_key_id(&p) {
                    println!("{} is a Share B for key {id}", p.display());
                }
            }
            Ok(())
        }
        "backup-share-a" => {
            let id = key_id_arg(&a)?;
            let out = need(a.out.clone(), "--out")?;
            let prot = hermes_platform::default_protector().map_err(|x| e(&x))?;
            let p1 = rpassword::prompt_password("Backup passphrase (min 12 chars): ")
                .map_err(|x| e(&x))?;
            let p2 = rpassword::prompt_password("Repeat passphrase: ").map_err(|x| e(&x))?;
            if p1 != p2 {
                return Err("passphrases differ".into());
            }
            let text = backup_share_a(&keys_dir, &id, &p1, KdfParams::STRONG, prot.as_ref())
                .map_err(|x| e(&x))?;
            if out.exists() {
                return Err("output file exists; refusing to overwrite".into());
            }
            std::fs::write(&out, text).map_err(|x| e(&x))?;
            println!("wrote {}. Store it away from the USB drive.", out.display());
            println!("NOTE: the signing key is deliberately not part of this backup (see docs/KEY_MANAGEMENT.md).");
            Ok(())
        }
        "restore-share-a" => {
            let backup = need(a.backup.clone(), "--backup")?;
            let prot = hermes_platform::default_protector().map_err(|x| e(&x))?;
            let text = std::fs::read_to_string(&backup).map_err(|x| e(&x))?;
            let pass = rpassword::prompt_password("Backup passphrase: ").map_err(|x| e(&x))?;
            let id = restore_share_a(&keys_dir, &text, &pass, prot.as_ref(), false)
                .map_err(|x| e(&x))?;
            println!("restored Share A for key {id} (re-protected for this Windows user)");
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            eprintln!("error: {m}");
            ExitCode::FAILURE
        }
    }
}
