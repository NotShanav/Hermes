//! hermes-dump --data-dir D --share-b F [--share-b F2 ...] [--from-us N] [--to-us N] [--limit N] [--verify]
use hermes_dump::{run, DumpOptions};
use std::path::PathBuf;

fn main() {
    let mut o = DumpOptions {
        data_dir: PathBuf::new(),
        share_b_files: vec![],
        from_us: None,
        to_us: None,
        limit: None,
        verify: false,
    };
    let default_dir = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Hermes"));
    let mut dir = default_dir;
    let mut it = std::env::args().skip(1);
    while let Some(f) = it.next() {
        let mut val = || {
            it.next()
                .unwrap_or_else(|| fail(&format!("{f} needs a value")))
        };
        match f.as_str() {
            "--data-dir" => dir = Some(val().into()),
            "--share-b" => o.share_b_files.push(val().into()),
            "--from-us" => {
                o.from_us = Some(
                    val()
                        .parse()
                        .unwrap_or_else(|_| fail("--from-us must be an integer")),
                )
            }
            "--to-us" => {
                o.to_us = Some(
                    val()
                        .parse()
                        .unwrap_or_else(|_| fail("--to-us must be an integer")),
                )
            }
            "--limit" => {
                o.limit = Some(
                    val()
                        .parse()
                        .unwrap_or_else(|_| fail("--limit must be an integer")),
                )
            }
            "--verify" => o.verify = true,
            other => fail(&format!("unknown flag {other}")),
        }
    }
    o.data_dir = dir.unwrap_or_else(|| fail("--data-dir is required"));
    let prot = hermes_platform::default_protector().unwrap_or_else(|e| fail(&e.to_string()));
    let mut out = std::io::stdout().lock();
    if let Err(e) = run(&o, prot.as_ref(), &mut out) {
        fail(&e);
    }
}

fn fail(m: &str) -> ! {
    eprintln!("error: {m}");
    std::process::exit(1);
}
