use hermes_db::ArchiveReader;
use hermes_viewer::fmt_time;
use std::path::Path;

/// Non-secret facts about the archive (no keys needed): shown by `hermes status`.
pub fn report(data_dir: &Path, offset_min: i32, collector_running: Option<bool>) -> String {
    let mut o = String::new();
    o.push_str(&format!("Data folder:   {}\n", data_dir.display()));
    let db = data_dir.join("hermes.db");
    match collector_running {
        Some(true) => o.push_str("Collector:     RUNNING (recording now)\n"),
        Some(false) => o.push_str("Collector:     not running\n"),
        None => o.push_str("Collector:     (can only be detected on Windows)\n"),
    }
    if !db.exists() {
        o.push_str("Archive:       none yet. Run `hermes init --usb E:\\` first.\n");
        return o;
    }
    let size = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0) + std::fs::metadata(data_dir.join("hermes.db-wal")).map(|m| m.len()).unwrap_or(0);
    o.push_str(&format!("Archive:       {} ({:.1} MiB)\n", db.display(), size as f64 / 1048576.0));
    match ArchiveReader::open(&db) {
        Err(e) => o.push_str(&format!("               cannot open: {e}\n")),
        Ok(rd) => {
            let keys = rd.keys().unwrap_or_default();
            let n = rd.count_events().unwrap_or(0);
            o.push_str(&format!("Schema:        v{}\n", rd.schema_version().unwrap_or(0)));
            for k in &keys {
                let a = hermes_crypto::shares::share_a_path(&data_dir.join("keys"), &k.info.key_id).exists();
                o.push_str(&format!(
                    "Key set:       {} {} signing:{} share A on this PC:{}\n",
                    k.info.key_id,
                    if k.active { "(active)" } else { "(retired)" },
                    if k.sign_public.is_some() { "yes" } else { "NO (old key set: run `hermes-keytool rotate`)" },
                    if a { "yes" } else { "MISSING" }
                ));
            }
            if keys.is_empty() {
                o.push_str("Key set:       none registered\n");
            }
            o.push_str(&format!("Events:        {n}\n"));
            match rd.time_range().ok().flatten() {
                Some((a, b)) => o.push_str(&format!("Time range:    {} .. {} (UTC{})\n", fmt_time(a, offset_min), fmt_time(b, offset_min), hermes_viewer::fmt_offset(offset_min))),
                None => o.push_str("Time range:    (empty archive: nothing recorded yet)\n"),
            }
            match rd.meta("clean_shutdown").ok().flatten().as_deref() {
                Some("1") => o.push_str("Last shutdown: clean\n"),
                Some(_) => o.push_str("Last shutdown: NOT clean (still running, or it was interrupted)\n"),
                None => {}
            }
        }
    }
    o.push_str("(This summary shows only non-secret metadata. Contents need both key shares: `hermes view`.)\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{init, InitOptions};
    use hermes_crypto::protector::TestProtector;

    #[test]
    fn status_before_and_after_init() {
        let d = tempfile::tempdir().unwrap();
        let s = report(&d.path().join("none"), 0, Some(false));
        assert!(s.contains("none yet") && s.contains("not running"));
        let usb = tempfile::tempdir().unwrap();
        let i = init(&InitOptions { data_dir: d.path(), share_b: &usb.path().join("k.key"), allow_same_drive: true }, &TestProtector::new()).unwrap();
        let s = report(d.path(), 0, None);
        assert!(s.contains(&i.key_id) && s.contains("(active)") && s.contains("signing:yes") && s.contains("share A on this PC:yes"), "{s}");
        assert!(s.contains("Events:        0") && s.contains("empty archive") && s.contains("Last shutdown: clean"), "{s}");
    }
}
