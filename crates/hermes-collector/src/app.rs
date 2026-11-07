use hermes_collector::console::{run, ConsoleOptions};
use std::path::PathBuf;

fn parse() -> Result<ConsoleOptions, String> {
    let mut it = std::env::args().skip(1);
    match it.next().as_deref() {
        Some("run") => {}
        _ => return Err("usage: hermes-collector run [--data-dir D] [--exclude app.exe]... [--no-default-exclusions] [--quiet] [--duration-secs N]\n(the `hermes` command is the easier front end: `hermes start`)".into()),
    }
    let default_dir = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Hermes")).unwrap_or_else(|| PathBuf::from("Hermes"));
    let mut o = ConsoleOptions { data_dir: default_dir, exclude: vec![], default_exclusions: true, quiet: false, duration_secs: None };
    while let Some(f) = it.next() {
        match f.as_str() {
            "--data-dir" => o.data_dir = it.next().ok_or("--data-dir needs a value")?.into(),
            "--exclude" => o.exclude.push(it.next().ok_or("--exclude needs a value")?),
            "--no-default-exclusions" => o.default_exclusions = false,
            "--quiet" => o.quiet = true,
            "--duration-secs" => o.duration_secs = Some(it.next().ok_or("--duration-secs needs a value")?.parse().map_err(|_| "--duration-secs must be a number")?),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(o)
}

pub fn main() -> i32 {
    match parse().and_then(run) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
