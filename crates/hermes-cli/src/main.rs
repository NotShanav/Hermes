//! `hermes`: the front end. Run `hermes help`.
use hermes_cli::args::Args;
use hermes_cli::{init, repl, status, usb};
use hermes_viewer::{date_range, Archive, Filter};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "\
Hermes Beta 0.1: a local, encrypted record of which applications and window titles you used.

  hermes init --usb E:\\                 first-time setup: keys, encrypted archive, USB key file
  hermes start [--quiet]                start recording (visible console; Ctrl+C or `hermes stop` ends it)
  hermes tray                           start recording with a system-tray icon (pause/resume/status)
  hermes stop                           stop a running collector gracefully
  hermes status                         what exists, whether it is running (no keys needed)
  hermes check [--share-b FILE]         database + integrity check (signatures need the USB key file)
  hermes view [--share-b FILE]          interactive history viewer (needs BOTH key shares)
  hermes search TEXT [--app A] [--date D] [--limit N]
  hermes export --out FILE [--format csv|json] [--app A] [--find TEXT] [--date D] [--force]

Common options:  --data-dir DIR (default %LOCALAPPDATA%\\Hermes)   --share-b FILE (repeat for several key sets)
                 --tz-offset-minutes N | --utc   (display/filter time zone; default: Windows local time)
Dates: today | yesterday | 2025-01-31 | 2025-01-01..2025-01-31
If --share-b is omitted Hermes looks for X:\\hermes-share-b.key on every drive, then asks.
";

fn data_dir(a: &Args) -> PathBuf {
    a.value("--data-dir").map(PathBuf::from).unwrap_or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Hermes")).unwrap_or_else(|| PathBuf::from("Hermes")))
}

fn offset(a: &Args) -> Result<i32, String> {
    if a.has("--utc") {
        return Ok(0);
    }
    match a.value("--tz-offset-minutes") {
        Some(v) => v.parse().map_err(|_| "--tz-offset-minutes must be a whole number".to_string()),
        None => Ok(usb::local_offset_minutes()),
    }
}

fn now_us() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0)
}

/// Explicit --share-b files, else files found on attached drives, else ask.
fn share_b_files(a: &Args, required: bool) -> Result<Vec<PathBuf>, String> {
    let explicit: Vec<PathBuf> = a.values("--share-b").into_iter().map(PathBuf::from).collect();
    if !explicit.is_empty() {
        return Ok(explicit);
    }
    let found = usb::auto_detect();
    if !found.is_empty() {
        for f in &found {
            eprintln!("Using USB key file {}", f.display());
        }
        return Ok(found);
    }
    if !required {
        return Ok(vec![]);
    }
    eprint!("Plug in your USB stick, then enter the path of the Hermes key file (e.g. E:\\hermes-share-b.key): ");
    std::io::stderr().flush().ok();
    let mut s = String::new();
    std::io::stdin().lock().read_line(&mut s).map_err(|e| e.to_string())?;
    let s = s.trim().trim_matches('"');
    if s.is_empty() {
        return Err("no key file given: both key shares are required to read the history".into());
    }
    Ok(vec![usb::share_b_target(s)])
}

fn filter_from(a: &Args, off: i32) -> Result<Filter, String> {
    let mut f = Filter { app: a.value("--app").map(String::from), text: a.value("--find").map(String::from), ..Default::default() };
    if let Some(d) = a.value("--date") {
        if let Some((from, to)) = date_range(d, off, now_us())? {
            f.from_us = Some(from);
            f.to_us = Some(to);
        }
    }
    Ok(f)
}

fn open_archive(a: &Args) -> Result<Archive, String> {
    let dir = data_dir(a);
    let prot = hermes_platform::default_protector().map_err(|e| format!("{e}\n(`hermes` reads Share A through Windows DPAPI: use it on the Windows account that recorded the history)"))?;
    let files = share_b_files(a, true)?;
    Archive::open(&dir, &files, prot.as_ref())
}

const COMMON: &[&str] = &["--data-dir", "--share-b", "--tz-offset-minutes"];

fn run() -> Result<u8, String> {
    let a = Args::parse(std::env::args().skip(1))?;
    if a.has("--help") || matches!(a.command.as_str(), "help" | "-h" | "--help") {
        print!("{HELP}");
        return Ok(0);
    }
    match a.command.as_str() {
        "init" => {
            a.check_known(&["--data-dir", "--usb"], &["--allow-same-drive"])?;
            let dir = data_dir(&a);
            let target = match a.value("--usb") {
                Some(u) => usb::share_b_target(u),
                None => match usb::auto_detect().first() {
                    Some(_) => return Err("a Hermes key file already exists on an attached drive; pass --usb explicitly".into()),
                    None => return Err("--usb is required: plug in the USB stick that will hold the second key share and run\n    hermes init --usb E:\\".into()),
                },
            };
            let prot = hermes_platform::default_protector().map_err(|e| e.to_string())?;
            let info = init::init(&init::InitOptions { data_dir: &dir, share_b: &target, allow_same_drive: a.has("--allow-same-drive") }, prot.as_ref())?;
            print!("{}", init::explain(&info));
            Ok(0)
        }
        "start" => {
            a.check_known(&["--data-dir", "--exclude", "--duration-secs"], &["--quiet", "--no-default-exclusions"])?;
            start(&a)
        }
        "tray" => {
            a.check_known(&["--data-dir"], &["--paused"])?;
            let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("hermes-tray.exe");
            if !exe.exists() {
                return Err(format!("{} not found next to hermes.exe", exe.display()));
            }
            let mut c = std::process::Command::new(exe);
            c.arg("--data-dir").arg(data_dir(&a));
            if a.has("--paused") {
                c.arg("--paused");
            }
            c.spawn().map_err(|e| e.to_string())?;
            println!("Hermes tray started. Look for the red recording icon in the notification area (it may be in the ^ overflow).");
            Ok(0)
        }
        "stop" => stop(),
        "status" => {
            a.check_known(&["--data-dir", "--tz-offset-minutes"], &["--utc"])?;
            print!("{}", status::report(&data_dir(&a), offset(&a)?, running()));
            Ok(0)
        }
        "check" => {
            a.check_known(COMMON, &["--utc"])?;
            check(&a)
        }
        "view" => {
            a.check_known(COMMON, &["--utc"])?;
            let off = offset(&a)?;
            let ar = open_archive(&a)?;
            let stdin = std::io::stdin();
            let mut out = std::io::stdout();
            repl::run(&ar, &repl::ReplConfig { offset_min: off, page: 25, now_us: now_us() }, stdin.lock(), &mut out).map_err(|e| e.to_string())?;
            Ok(0)
        }
        "search" => {
            a.check_known(&["--data-dir", "--share-b", "--tz-offset-minutes", "--app", "--date", "--limit"], &["--utc"])?;
            let off = offset(&a)?;
            let text = a.positionals.join(" ");
            if text.is_empty() && a.value("--app").is_none() && a.value("--date").is_none() {
                return Err("usage: hermes search TEXT [--app A] [--date D] [--limit N]".into());
            }
            let mut f = filter_from(&a, off)?;
            if !text.is_empty() {
                f.text = Some(text);
            }
            let limit: usize = a.value("--limit").map(|v| v.parse().map_err(|_| "--limit must be a number".to_string())).transpose()?.unwrap_or(200);
            let ar = open_archive(&a)?;
            let r = ar.select(&f);
            for e in r.iter().take(limit) {
                println!("{}", repl::format_event(e, off));
            }
            println!("{} match(es){}", r.len(), if r.len() > limit { format!(", first {limit} shown (--limit)") } else { String::new() });
            Ok(0)
        }
        "export" => {
            a.check_known(&["--data-dir", "--share-b", "--tz-offset-minutes", "--out", "--format", "--app", "--find", "--date"], &["--utc", "--force"])?;
            let off = offset(&a)?;
            let out = a.value("--out").ok_or("--out FILE is required")?;
            let fmt = a.value("--format").unwrap_or_else(|| if out.to_lowercase().ends_with(".json") { "json" } else { "csv" });
            if fmt != "csv" && fmt != "json" {
                return Err("--format must be csv or json".into());
            }
            let f = filter_from(&a, off)?;
            let ar = open_archive(&a)?;
            let r = ar.select(&f);
            let n = repl::write_export(&r, fmt, out, off, a.has("--force"))?;
            println!("Exported {n} event(s) to {out}\n{}", repl::PLAINTEXT_WARNING);
            Ok(0)
        }
        other => Err(format!("unknown command `{other}`. Run `hermes help`.")),
    }
}

fn check(a: &Args) -> Result<u8, String> {
    use hermes_db::ArchiveReader;
    let dir = data_dir(a);
    let db = dir.join("hermes.db");
    if !db.exists() {
        return Err(format!("no archive at {}", db.display()));
    }
    let rd = ArchiveReader::open(&db).map_err(|e| e.to_string())?;
    let mut ok = true;
    let qc = rd.quick_check().map_err(|e| e.to_string())?;
    println!("SQLite structure check: {qc}");
    ok &= qc == "ok";
    let gaps = rd.seq_gaps().map_err(|e| e.to_string())?;
    println!("Sequence numbers: {}", if gaps.is_empty() { "no gaps".to_string() } else { format!("MISSING {gaps:?}") });
    ok &= gaps.is_empty();
    let files = share_b_files(a, false)?;
    let mut trust = std::collections::HashMap::new();
    for f in &files {
        match hermes_crypto::shares::parse_share_b(&std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?) {
            Ok(p) => {
                if let Some(sp) = p.sign_public {
                    trust.insert(hermes_crypto::sign::signer_id(&sp), sp);
                }
            }
            Err(e) => println!("{}: not a usable key file ({e})", f.display()),
        }
    }
    let rep = rd.verify_integrity(&trust).map_err(|e| e.to_string())?;
    if trust.is_empty() {
        println!("Signatures: NOT CHECKED (plug in the USB stick or pass --share-b; only its signing key is needed, not Share A)");
    } else {
        println!("Signed checkpoints: {} of {} verified", rep.checkpoints_verified, rep.checkpoints_total);
        for (n, v) in [("modified ranges", format!("{:?}", rep.chain_mismatch)), ("bad signatures", format!("{:?}", rep.bad_signature)), ("broken links", format!("{:?}", rep.link_breaks)), ("unsigned records", format!("{:?}", rep.unsigned_rows))] {
            if !v.ends_with("[]") {
                println!("  PROBLEM {n}: {v}");
            }
        }
        ok &= rep.is_clean();
    }
    if rep.legacy_unsigned_rows > 0 {
        println!("Legacy (pre-signing) records: {} (cannot be signed)", rep.legacy_unsigned_rows);
    }
    println!("Not detectable by this check: deletion of the newest records, or restoring an older copy of the archive.");
    println!("RESULT: {}", if ok && !trust.is_empty() { "OK" } else if ok { "STRUCTURE OK (signatures not verified)" } else { "PROBLEMS FOUND" });
    Ok(if ok { 0 } else { 2 })
}

#[cfg(windows)]
fn running() -> Option<bool> {
    Some(hermes_tracker::win::running_instance().is_some())
}
#[cfg(not(windows))]
fn running() -> Option<bool> {
    None
}

#[cfg(windows)]
fn start(a: &Args) -> Result<u8, String> {
    use hermes_collector::console::{run, ConsoleOptions};
    run(ConsoleOptions {
        data_dir: data_dir(a),
        exclude: a.values("--exclude"),
        default_exclusions: !a.has("--no-default-exclusions"),
        quiet: a.has("--quiet"),
        duration_secs: a.value("--duration-secs").map(|v| v.parse().map_err(|_| "--duration-secs must be a number".to_string())).transpose()?,
    })?;
    Ok(0)
}
#[cfg(not(windows))]
fn start(_a: &Args) -> Result<u8, String> {
    Err("recording needs Windows (the foreground tracker uses Windows APIs)".into())
}

#[cfg(windows)]
fn stop() -> Result<u8, String> {
    match hermes_tracker::win::running_instance() {
        None => {
            println!("Hermes is not running.");
            Ok(0)
        }
        Some(h) => {
            h.stop();
            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if hermes_tracker::win::running_instance().is_none() {
                    println!("Hermes stopped (everything was flushed and the archive closed cleanly).");
                    return Ok(0);
                }
            }
            Err("asked Hermes to stop but it is still running after 10 s".into())
        }
    }
}
#[cfg(not(windows))]
fn stop() -> Result<u8, String> {
    Err("`hermes stop` needs Windows".into())
}

fn main() -> ExitCode {
    match run() {
        Ok(c) => ExitCode::from(c),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
