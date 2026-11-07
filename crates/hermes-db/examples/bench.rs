//! Synthetic pipeline benchmark (encrypt + sign + batch-write). Prints timing and (on Linux) RSS.
//! This is NOT a measurement of the Windows collector: see docs/WINDOWS_TESTING.md for that.
//!   cargo run --release -p hermes-db --example bench -- [events]
use hermes_crypto::{generate_key_set, RecordSigner};
use hermes_db::{Database, EventStore, OpenOptions, StoreConfig};
use std::time::Instant;

fn rss_mib() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = s
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb / 1024.0)
}
fn hwm_mib() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = s
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb / 1024.0)
}

fn main() {
    let n: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);
    let dir = std::env::temp_dir().join(format!("hermes-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hermes.db");
    let ks = generate_key_set().unwrap();
    let (signer, seed) = RecordSigner::generate().unwrap();
    {
        let (mut db, _) = Database::open(&path, OpenOptions::default()).unwrap();
        db.register_key(&ks.info, Some(&signer.public()), true)
            .unwrap();
        db.mark_clean_shutdown().unwrap();
    }
    println!("rss before: {:.1} MiB", rss_mib().unwrap_or(0.0));
    let body = br#"{"v":1,"mono_us":123456789,"t":"focus_gained","app":{"pid":1234,"process_start":133000000000000000,"exe_path":"C:\\Program Files\\Some App\\app.exe","exe_name":"app.exe","status":"resolved"},"window":{"hwnd":65722,"class":"Chrome_WidgetWin_1","title":"Quarterly report - Document - Some Application","title_status":"text"}}"#;
    let mut st = EventStore::open(
        &path,
        RecordSigner::from_seed(&seed),
        StoreConfig::default(),
    )
    .unwrap();
    let t = Instant::now();
    for i in 0..n {
        st.push(7, i as i64, 0x110, body).unwrap();
    }
    st.flush().unwrap();
    let el = t.elapsed();
    println!(
        "{n} events (body {} bytes): {:.2?} total = {:.1} us/event = {:.0} events/s",
        body.len(),
        el,
        el.as_micros() as f64 / n as f64,
        n as f64 / el.as_secs_f64()
    );
    println!(
        "rss after: {:.1} MiB   peak rss: {:.1} MiB",
        rss_mib().unwrap_or(0.0),
        hwm_mib().unwrap_or(0.0)
    );
    let sz = std::fs::metadata(&path).unwrap().len()
        + std::fs::metadata(dir.join("hermes.db-wal"))
            .map(|m| m.len())
            .unwrap_or(0);
    println!(
        "on disk: {:.1} MiB = {:.0} bytes/event",
        sz as f64 / 1048576.0,
        sz as f64 / n as f64
    );
    st.close().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
