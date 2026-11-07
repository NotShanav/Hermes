//! Prints the decrypted archive. This is verification tooling for the collector milestones; the real
//! Viewer (search, filters, UI) is milestone 6. It requires both key shares and prints PLAINTEXT to
//! the console: do not run it where others can see the screen or capture the output.

use hermes_crypto::provision::load_viewer_material;
use hermes_crypto::shares::peek_share_b_key_id;
use hermes_crypto::{Opener, SecretProtector};
use hermes_db::{kind, ArchiveReader, EventQuery, IntegrityReport, Keyring, OpenEventError};
use hermes_tracker::{decode_body, kinds, ActivityEvent};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

pub struct DumpOptions {
    pub data_dir: PathBuf,
    /// One Share-B file per key set whose history should be unlocked.
    pub share_b_files: Vec<PathBuf>,
    pub from_us: Option<i64>,
    pub to_us: Option<i64>,
    pub limit: Option<u32>,
    pub verify: bool,
}

#[derive(Debug, Default)]
pub struct DumpSummary {
    pub events_total: u64,
    pub decrypted: u64,
    pub locked: u64,
    pub tampered: u64,
    pub integrity: Option<IntegrityReport>,
    pub seq_gaps: Vec<(u64, u64)>,
}

/// Microseconds since the Unix epoch -> "YYYY-MM-DD HH:MM:SS.ffffff UTC" (civil-from-days algorithm).
pub fn fmt_ts(us: i64) -> String {
    let secs = us.div_euclid(1_000_000);
    let frac = us.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{frac:06} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn describe(kind_: u16, body: &[u8]) -> String {
    match kind_ {
        kind::SYSTEM_GAP => format!("SYSTEM_GAP       {}", String::from_utf8_lossy(body)),
        kind::SYSTEM_RECOVERED => format!("SYSTEM_RECOVERED {}", String::from_utf8_lossy(body)),
        kinds::SESSION_START
        | kinds::SESSION_END
        | kinds::FOCUS_GAINED
        | kinds::FOCUS_LOST
        | kinds::TITLE_CHANGED => match decode_body(body) {
            Ok(b) => match b.event {
                ActivityEvent::SessionStart { reason, version } => {
                    format!("SESSION_START    reason={reason:?} collector={version}")
                }
                ActivityEvent::SessionEnd { reason } => {
                    format!("SESSION_END      reason={reason:?}")
                }
                ActivityEvent::FocusGained {
                    app,
                    window,
                    from_hwnd,
                } => format!(
                    "FOCUS_GAINED     {} pid={} hwnd={:#x} class={} title={:?} [{:?}/{:?}]{}",
                    app.exe_name.as_deref().unwrap_or("<unknown>"),
                    app.pid,
                    window.hwnd,
                    window.class.as_deref().unwrap_or("-"),
                    window.title.as_deref().unwrap_or(""),
                    app.status,
                    window.title_status,
                    from_hwnd
                        .map(|h| format!(" from={h:#x}"))
                        .unwrap_or_default()
                ),
                ActivityEvent::FocusLost {
                    reason,
                    hwnd,
                    pid,
                    focus_us,
                } => format!(
                    "FOCUS_LOST       hwnd={hwnd:#x} pid={pid} reason={reason:?} held={:.3}s",
                    focus_us as f64 / 1e6
                ),
                ActivityEvent::TitleChanged {
                    hwnd,
                    title,
                    title_status,
                    ..
                } => format!(
                    "TITLE_CHANGED    hwnd={hwnd:#x} title={:?} [{title_status:?}]",
                    title.unwrap_or_default()
                ),
            },
            Err(e) => format!("kind {kind_:#06x}: body not decodable ({e})"),
        },
        other => format!("kind {other:#06x}: {} bytes", body.len()),
    }
}

pub fn run(
    opts: &DumpOptions,
    protector: &dyn SecretProtector,
    out: &mut dyn Write,
) -> Result<DumpSummary, String> {
    let e = |x: &dyn std::fmt::Display| x.to_string();
    let rd = ArchiveReader::open(&opts.data_dir.join("hermes.db")).map_err(|x| e(&x))?;
    let registered = rd.keys().map_err(|x| e(&x))?;
    let keys_dir = opts.data_dir.join("keys");

    let mut keyring = Keyring::new();
    let mut trust: HashMap<[u8; 16], [u8; 32]> = HashMap::new();
    for p in &opts.share_b_files {
        let id = peek_share_b_key_id(p).map_err(|x| format!("{}: {x}", p.display()))?;
        let (key, sign_pub) = load_viewer_material(&keys_dir, &id, p, protector)
            .map_err(|x| format!("key {id}: {x}"))?;
        writeln!(
            out,
            "# unlocked key set {id} (signing anchor from USB: {})",
            if sign_pub.is_some() {
                "yes"
            } else {
                "none (pre-signing key set)"
            }
        )
        .ok();
        if let Some(sp) = sign_pub {
            trust.insert(hermes_crypto::sign::signer_id(&sp), sp);
        }
        keyring.add(Opener::new(key));
    }
    for k in &registered {
        if !keyring.has(&k.info.key_id) {
            writeln!(
                out,
                "# key set {} is LOCKED (no Share B supplied): its events cannot be shown",
                k.info.key_id
            )
            .ok();
        }
    }

    let mut sum = DumpSummary::default();
    let mut after = None;
    let mut remaining = opts.limit.map(|l| l as u64);
    'outer: loop {
        let page = rd
            .events(&EventQuery {
                from_us: opts.from_us,
                to_us: opts.to_us,
                after_seq: after,
                limit: Some(1000),
                descending: false,
            })
            .map_err(|x| e(&x))?;
        if page.is_empty() {
            break;
        }
        for ev in &page {
            after = Some(ev.seq);
            sum.events_total += 1;
            let line = match keyring.open_event(ev) {
                Ok(d) => {
                    sum.decrypted += 1;
                    format!(
                        "{}{}  S:{:016x}  {}",
                        if d.legacy { "L " } else { "  " },
                        "",
                        d.session_id,
                        describe(d.kind, &d.body)
                    )
                }
                Err(OpenEventError::MissingKey(id)) => {
                    sum.locked += 1;
                    format!("  [LOCKED: key set {id} not unlocked]")
                }
                Err(OpenEventError::Tampered) => {
                    sum.tampered += 1;
                    "  [TAMPERED OR DAMAGED: authentication failed]".to_string()
                }
                Err(OpenEventError::Other(x)) => format!("  [error: {x}]"),
            };
            writeln!(out, "#{:<7} {}  {}", ev.seq, fmt_ts(ev.ts_us), line).map_err(|x| e(&x))?;
            if let Some(r) = remaining.as_mut() {
                *r -= 1;
                if *r == 0 {
                    break 'outer;
                }
            }
        }
    }
    sum.seq_gaps = rd.seq_gaps().map_err(|x| e(&x))?;
    for (a, b) in &sum.seq_gaps {
        writeln!(
            out,
            "# MISSING sequence numbers {a}..={b} (rows deleted or lost in a repair)"
        )
        .ok();
    }
    if opts.verify {
        let rep = rd.verify_integrity(&trust).map_err(|x| e(&x))?;
        writeln!(
            out,
            "# integrity: {}/{} signed checkpoints verified; chain mismatches {:?}; bad signatures {:?}; link breaks {:?}; unsigned rows {:?}; legacy unsigned rows {}",
            rep.checkpoints_verified, rep.checkpoints_total, rep.chain_mismatch, rep.bad_signature, rep.link_breaks, rep.unsigned_rows, rep.legacy_unsigned_rows
        )
        .ok();
        if trust.is_empty() {
            writeln!(out, "# integrity: NO trusted verifying key available (supply a v2 Share B): signatures could not be checked").ok();
        }
        writeln!(
            out,
            "# integrity verdict: {}",
            if rep.is_clean() && !trust.is_empty() {
                "CLEAN"
            } else {
                "PROBLEMS FOUND OR UNVERIFIABLE"
            }
        )
        .ok();
        sum.integrity = Some(rep);
    }
    writeln!(
        out,
        "# {} events, {} decrypted, {} locked, {} tampered",
        sum.events_total, sum.decrypted, sum.locked, sum.tampered
    )
    .ok();
    Ok(sum)
}
