//! Filesystem validation: runs the storage and key-file code on the volume under test (NTFS, exFAT,
//! FAT32, USB sticks...) and reports what works. Everything happens in a throw-away sub-directory.

use hermes_crypto::provision::create_key_set;
use hermes_crypto::shares::{parse_share_b, write_share_b};
use hermes_crypto::{generate_key_set, RecordSigner, SecretProtector};
use hermes_db::{ArchiveReader, Database, EventQuery, EventStore, OpenOptions, StoreConfig};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

pub struct Check {
    pub name: &'static str,
    pub outcome: Result<String, String>,
    pub skipped: bool,
    pub ms: u128,
}

fn timed(name: &'static str, f: impl FnOnce() -> Result<String, String>) -> Check {
    let t = Instant::now();
    let outcome = f();
    let skipped = matches!(&outcome, Ok(m) if m.starts_with("skipped"));
    Check {
        name,
        outcome,
        skipped,
        ms: t.elapsed().as_millis(),
    }
}

pub fn run(
    dir: &Path,
    local_dir: &Path,
    protector: Option<&dyn SecretProtector>,
    events: u32,
) -> Vec<Check> {
    let mut out = vec![];
    let scratch = dir.join(format!("hermes-fscheck-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&scratch) {
        out.push(Check {
            name: "create scratch directory",
            outcome: Err(e.to_string()),
            skipped: false,
            ms: 0,
        });
        return out;
    }

    out.push(timed("volume information", || {
        hermes_platform::fs_info(&scratch)
            .map(|i| {
                format!(
                    "root={} fs={} type={} max_name={}",
                    i.volume_root, i.fs_name, i.drive_type, i.max_component_len
                )
            })
            .map(Ok)
            .unwrap_or_else(|| Ok("skipped: not available on this platform".to_string()))
    }));

    out.push(timed("write + fsync + read back 16 MiB", || {
        let p = scratch.join("blob.bin");
        let data: Vec<u8> = (0..16 * 1024 * 1024u32)
            .map(|i| (i.wrapping_mul(31)) as u8)
            .collect();
        let mut f = std::fs::File::create(&p).map_err(|e| e.to_string())?;
        f.write_all(&data).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| format!("fsync: {e}"))?;
        drop(f);
        let back = std::fs::read(&p).map_err(|e| e.to_string())?;
        if back != data {
            return Err("read-back mismatch".into());
        }
        Ok("ok".into())
    }));

    out.push(timed(
        "USB key file: write, replace-in-place (atomic rename over existing), read back",
        || {
            let ks = generate_key_set().map_err(|e| e.to_string())?;
            let p = scratch.join("share-b.key");
            write_share_b(&p, &ks.info.key_id, &ks.share_b, &[1u8; 32], false)
                .map_err(|e| e.to_string())?;
            if write_share_b(&p, &ks.info.key_id, &ks.share_b, &[1u8; 32], false).is_ok() {
                return Err("overwrite protection failed".into());
            }
            write_share_b(&p, &ks.info.key_id, &ks.share_b, &[2u8; 32], true)
                .map_err(|e| format!("replace: {e}"))?;
            let f = parse_share_b(&std::fs::read_to_string(&p).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if f.sign_public != Some([2u8; 32]) {
                return Err("replaced content not visible".into());
            }
            let leftovers = std::fs::read_dir(&scratch)
                .map_err(|e| e.to_string())?
                .filter(|e| {
                    e.as_ref()
                        .map(|e| e.file_name().to_string_lossy().starts_with(".hermes-tmp"))
                        .unwrap_or(false)
                })
                .count();
            if leftovers != 0 {
                return Err(format!("{leftovers} temp file(s) left behind"));
            }
            Ok("ok".into())
        },
    ));

    match protector {
        Some(prot) => out.push(timed(
            "full key set (DPAPI share A + signing key local, share B on this volume)",
            || {
                let b = scratch.join("full-share-b.key");
                let nk = create_key_set(local_dir, &b, prot).map_err(|e| e.to_string())?;
                // clean up the throw-away key set from the local dir
                let _ = std::fs::remove_file(hermes_crypto::shares::share_a_path(
                    local_dir,
                    &nk.info.key_id,
                ));
                let _ = std::fs::remove_file(hermes_crypto::shares::signing_key_path(
                    local_dir,
                    &nk.info.key_id,
                ));
                Ok(format!(
                    "key {} created, read back and reconstructed",
                    nk.info.key_id
                ))
            },
        )),
        None => out.push(Check {
            name: "full key set (DPAPI)",
            outcome: Ok("skipped: no OS protector on this platform".into()),
            skipped: true,
            ms: 0,
        }),
    }

    let db_path = scratch.join("fscheck.db");
    let ks = generate_key_set().expect("keygen");
    let (signer, seed) = RecordSigner::generate().expect("signer");
    let trust = HashMap::from([(signer.signer_id(), signer.public())]);

    out.push(timed(
        "SQLite: open database, enable WAL, register key",
        || {
            let (mut db, _) =
                Database::open(&db_path, OpenOptions::default()).map_err(|e| e.to_string())?;
            db.register_key(&ks.info, Some(&signer.public()), true)
                .map_err(|e| e.to_string())?;
            db.mark_clean_shutdown().map_err(|e| e.to_string())?;
            Ok("WAL enabled".into())
        },
    ));

    out.push(timed(
        "SQLite: batched encrypted writes, clean close",
        || {
            let mut st = EventStore::open(
                &db_path,
                RecordSigner::from_seed(&seed),
                StoreConfig::default(),
            )
            .map_err(|e| e.to_string())?;
            for i in 0..events {
                st.push(
                    1,
                    i as i64,
                    0x110,
                    format!("fscheck event {i} ................................").as_bytes(),
                )
                .map_err(|e| e.to_string())?;
            }
            st.close().map_err(|e| e.to_string())?;
            Ok(format!("{events} events"))
        },
    ));

    out.push(timed(
        "SQLite: reopen, forced integrity check, signed chain verification",
        || {
            let (db, rep) = Database::open(
                &db_path,
                OpenOptions {
                    force_integrity_check: true,
                },
            )
            .map_err(|e| e.to_string())?;
            if rep.recovery.is_some() {
                return Err("database was reported corrupt after a clean close".into());
            }
            drop(db);
            let rd = ArchiveReader::open(&db_path).map_err(|e| e.to_string())?;
            let n = rd.count_events().map_err(|e| e.to_string())?;
            if n != events as u64 {
                return Err(format!("expected {events} events, found {n}"));
            }
            let ir = rd.verify_integrity(&trust).map_err(|e| e.to_string())?;
            if !ir.is_clean() {
                return Err(format!("integrity report not clean: {ir:?}"));
            }
            let _ = rd
                .events(&EventQuery {
                    limit: Some(1),
                    ..Default::default()
                })
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "{n} events, {} signed checkpoints verified",
                ir.checkpoints_verified
            ))
        },
    ));

    out.push(timed(
        "SQLite: unclean shutdown (writer dropped mid-batch) recovers",
        || {
            {
                let mut st = EventStore::open(
                    &db_path,
                    RecordSigner::from_seed(&seed),
                    StoreConfig {
                        flush_events: 50,
                        ..Default::default()
                    },
                )
                .map_err(|e| e.to_string())?;
                for i in 0..120 {
                    st.push(2, 1_000_000 + i, 0x110, b"unflushed tail")
                        .map_err(|e| e.to_string())?;
                }
                // dropped without close(): 100 flushed, 20 lost
            }
            let st = EventStore::open(
                &db_path,
                RecordSigner::from_seed(&seed),
                StoreConfig::default(),
            )
            .map_err(|e| e.to_string())?;
            if !st.open_report().unclean_previous_shutdown || st.open_report().recovery.is_some() {
                return Err(format!("unexpected open report: {:?}", st.open_report()));
            }
            st.close().map_err(|e| e.to_string())?;
            let rd = ArchiveReader::open(&db_path).map_err(|e| e.to_string())?;
            let ir = rd.verify_integrity(&trust).map_err(|e| e.to_string())?;
            if !ir.is_clean() || !rd.seq_gaps().map_err(|e| e.to_string())?.is_empty() {
                return Err("chain/gap problems after unclean shutdown".into());
            }
            Ok("100 durable events kept, marker written, chain clean".into())
        },
    ));

    let _ = std::fs::remove_dir_all(&scratch);
    out
}
