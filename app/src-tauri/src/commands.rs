//! Tauri commands: the only surface the UI can call. Everything reads through `hermes-viewer`; nothing here
//! writes to the archive. Outgoing structs use snake_case keys; command arguments follow Tauri's camelCase rule.

use crate::collector;
use crate::prefs::{CategoryDef, Prefs, BUILTIN};
use crate::queries::{self, FocusRow, Summary, Timeline};
use crate::segments::build_segments;
use crate::state::{now_us, AppState, Session};
use hermes_db::ArchiveReader;
use hermes_tray::live::{self, LiveStatus};
use hermes_tray::settings::Settings;
use hermes_viewer::{export_csv, export_json, Archive, Filter, ViewEvent};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::State;

type Res<T> = Result<T, String>;

fn blocking_err(e: tauri::Error) -> String {
    e.to_string()
}

// ------------------------------------------------------------------------------------------------ app info

#[derive(Serialize)]
pub struct AppInfo {
    pub data_dir: String,
    pub has_archive: bool,
    pub unlocked: bool,
    pub offset_minutes: i32,
    pub version: &'static str,
    pub windows: bool,
    pub prefs: Prefs,
    pub categories: Vec<CategoryDef>,
}

fn categories_of(p: &Prefs) -> Vec<CategoryDef> {
    let mut v: Vec<CategoryDef> = BUILTIN.iter().map(|(id, name, color)| CategoryDef { id: (*id).into(), name: (*name).into(), color: (*color).into() }).collect();
    v.extend(p.custom_categories.iter().cloned());
    v
}

#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> Res<AppInfo> {
    let prefs = state.prefs.lock().unwrap().clone();
    Ok(AppInfo {
        data_dir: state.data_dir.display().to_string(),
        has_archive: state.data_dir.join("hermes.db").exists(),
        unlocked: state.session.read().unwrap().is_some(),
        offset_minutes: state.offset(),
        version: env!("CARGO_PKG_VERSION"),
        windows: cfg!(windows),
        categories: categories_of(&prefs),
        prefs,
    })
}

#[tauri::command]
pub async fn save_prefs(state: State<'_, AppState>, prefs: Prefs) -> Res<AppInfo> {
    prefs.save(&state.prefs_path()).map_err(|e| format!("could not save preferences: {e}"))?;
    *state.prefs.lock().unwrap() = prefs;
    get_app_info(state).await
}

// ------------------------------------------------------------------------------------------------ archive stats (no keys needed)

#[derive(Serialize, Clone)]
pub struct KeySetInfo {
    pub id: String,
    pub active: bool,
    pub signing: bool,
    pub share_a_present: bool,
    pub created_ms: i64,
}

#[derive(Serialize, Clone, Default)]
pub struct ArchiveStats {
    pub exists: bool,
    pub data_dir: String,
    pub events: u64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub size_bytes: u64,
    pub schema: u32,
    pub clean_shutdown: Option<bool>,
    pub keys: Vec<KeySetInfo>,
}

pub fn read_stats(dir: &Path) -> ArchiveStats {
    let db = dir.join("hermes.db");
    let mut st = ArchiveStats { data_dir: dir.display().to_string(), ..Default::default() };
    if !db.exists() {
        return st;
    }
    st.exists = true;
    let size = |p: PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    st.size_bytes = size(db.clone()) + size(dir.join("hermes.db-wal"));
    if let Ok(rd) = ArchiveReader::open(&db) {
        st.events = rd.count_events().unwrap_or(0);
        st.schema = rd.schema_version().unwrap_or(0);
        if let Ok(Some((a, b))) = rd.time_range() {
            st.first_ms = Some(a / 1000);
            st.last_ms = Some(b / 1000);
        }
        st.clean_shutdown = rd.meta("clean_shutdown").ok().flatten().map(|v| v == "1");
        for k in rd.keys().unwrap_or_default() {
            st.keys.push(KeySetInfo {
                id: k.info.key_id.to_hex(),
                active: k.active,
                signing: k.sign_public.is_some(),
                share_a_present: hermes_crypto::shares::share_a_path(&dir.join("keys"), &k.info.key_id).exists(),
                created_ms: k.created_us / 1000,
            });
        }
    }
    st
}

fn cached_stats(state: &AppState) -> ArchiveStats {
    let mut c = state.stats_cache.lock().unwrap();
    if let Some((at, s)) = c.as_ref() {
        if at.elapsed() < Duration::from_secs(3) {
            return s.clone();
        }
    }
    let s = read_stats(&state.data_dir);
    *c = Some((Instant::now(), s.clone()));
    s
}

#[tauri::command]
pub async fn lock_info(state: State<'_, AppState>) -> Res<ArchiveStats> {
    Ok(cached_stats(&state))
}

#[tauri::command]
pub async fn detect_keys() -> Res<Vec<String>> {
    Ok(hermes_cli::usb::auto_detect().into_iter().map(|p| p.display().to_string()).collect())
}

// ------------------------------------------------------------------------------------------------ setup, unlock, lock

#[derive(Serialize)]
pub struct InitOut {
    pub key_id: String,
    pub share_b: String,
}

#[tauri::command]
pub async fn init_archive(state: State<'_, AppState>, usb: String) -> Res<InitOut> {
    let dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> Res<InitOut> {
        let target = hermes_cli::usb::share_b_target(usb.trim().trim_matches('"'));
        let prot = hermes_platform::default_protector().map_err(|e| e.to_string())?;
        let info = hermes_cli::init::init(&hermes_cli::init::InitOptions { data_dir: &dir, share_b: &target, allow_same_drive: false }, prot.as_ref())?;
        Ok(InitOut { key_id: info.key_id, share_b: info.share_b.display().to_string() })
    })
    .await
    .map_err(blocking_err)?
}

#[derive(Serialize)]
pub struct IntegrityOut {
    pub clean: bool,
    pub summary: String,
    pub checkpoints_total: u64,
    pub checkpoints_verified: u64,
    pub tampered: u64,
    pub locked: u64,
    pub legacy_rows: u64,
    pub seq_gaps: Vec<(u64, u64)>,
    pub trust_anchors: usize,
    pub registered_keys: usize,
    pub unlocked_keys: usize,
    pub notes: Vec<String>,
    pub events: usize,
    pub opened_ms: i64,
}

fn integrity_out(s: &Session) -> IntegrityOut {
    let a = &s.archive;
    let (clean, summary) = hermes_cli::repl::integrity_summary(a);
    IntegrityOut {
        clean,
        summary,
        checkpoints_total: a.integrity.checkpoints_total,
        checkpoints_verified: a.integrity.checkpoints_verified,
        tampered: a.tampered,
        locked: a.locked,
        legacy_rows: a.legacy_rows,
        seq_gaps: a.seq_gaps.clone(),
        trust_anchors: a.trust_anchors,
        registered_keys: a.registered_keys,
        unlocked_keys: a.unlocked_keys,
        notes: a.notes.clone(),
        events: a.events.len(),
        opened_ms: s.opened_us / 1000,
    }
}

fn open_session(dir: PathBuf, files: Vec<PathBuf>) -> Res<Session> {
    let prot = hermes_platform::default_protector().map_err(|e| format!("{e}\n(Hermes reads Share A through Windows DPAPI: use the Windows account that recorded the history.)"))?;
    let archive = Archive::open(&dir, &files, prot.as_ref())?;
    let segs = build_segments(&archive.events);
    Ok(Session { archive, segs, key_files: files, opened_us: now_us() })
}

#[tauri::command]
pub async fn unlock(state: State<'_, AppState>, files: Vec<String>) -> Res<IntegrityOut> {
    if files.is_empty() {
        return Err("Choose the Hermes key file from your USB drive first.".into());
    }
    let dir = state.data_dir.clone();
    let paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(f.trim().trim_matches('"'))).collect();
    let session = tauri::async_runtime::spawn_blocking(move || open_session(dir, paths)).await.map_err(blocking_err)??;
    let out = integrity_out(&session);
    *state.session.write().unwrap() = Some(Arc::new(session));
    Ok(out)
}

#[tauri::command]
pub async fn lock_archive(state: State<'_, AppState>) -> Res<()> {
    *state.session.write().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub async fn integrity_info(state: State<'_, AppState>) -> Res<IntegrityOut> {
    Ok(integrity_out(&state.session()?))
}

/// Re-reads and re-verifies the whole archive with the same key files (picks up newly recorded events too).
#[tauri::command]
pub async fn recheck(state: State<'_, AppState>) -> Res<IntegrityOut> {
    let old = state.session()?;
    let dir = state.data_dir.clone();
    let files = old.key_files.clone();
    let session = tauri::async_runtime::spawn_blocking(move || open_session(dir, files)).await.map_err(blocking_err)??;
    let out = integrity_out(&session);
    *state.session.write().unwrap() = Some(Arc::new(session));
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ analytics

#[tauri::command]
pub async fn summary(state: State<'_, AppState>, from_ms: i64, to_ms: i64) -> Res<Summary> {
    let s = state.session()?;
    let rules = state.rules();
    Ok(queries::summarize(s.segs.iter(), &rules, state.offset(), from_ms * 1000, to_ms * 1000))
}

#[tauri::command]
pub async fn timeline(state: State<'_, AppState>, from_ms: i64, to_ms: i64) -> Res<Timeline> {
    let s = state.session()?;
    let rules = state.rules();
    Ok(queries::timeline(&s.segs, &rules, from_ms * 1000, to_ms * 1000, 60_000))
}

#[tauri::command]
pub async fn focus_list(state: State<'_, AppState>, from_ms: i64, to_ms: i64, app: Option<String>, limit: usize) -> Res<Vec<FocusRow>> {
    let s = state.session()?;
    let rules = state.rules();
    let it = s.segs.iter().filter(|x| app.as_ref().map_or(true, |a| &x.app == a));
    Ok(queries::focus_rows(it, &rules, from_ms * 1000, to_ms * 1000, limit.min(2000)))
}

#[derive(Serialize)]
pub struct AppDetail {
    pub summary: Summary,
    pub focus: Vec<FocusRow>,
    pub heat: Vec<Vec<i64>>,
}

#[tauri::command]
pub async fn app_detail(state: State<'_, AppState>, app: String, from_ms: i64, to_ms: i64) -> Res<AppDetail> {
    let s = state.session()?;
    let rules = state.rules();
    let off = state.offset();
    let (f, t) = (from_ms * 1000, to_ms * 1000);
    Ok(AppDetail {
        summary: queries::summarize(s.segs.iter().filter(|x| x.app == app), &rules, off, f, t),
        focus: queries::focus_rows(s.segs.iter().filter(|x| x.app == app), &rules, f, t, 200),
        heat: queries::heat(s.segs.iter().filter(|x| x.app == app), off, f, t),
    })
}

#[tauri::command]
pub async fn heatmap(state: State<'_, AppState>, from_ms: i64, to_ms: i64) -> Res<Vec<Vec<i64>>> {
    let s = state.session()?;
    Ok(queries::heat(s.segs.iter(), state.offset(), from_ms * 1000, to_ms * 1000))
}

// ------------------------------------------------------------------------------------------------ explore + export

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct FilterDto {
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub app: Option<String>,
    pub text: Option<String>,
    /// Event labels such as FOCUS, FOCUS END, TITLE, SESSION START, SYSTEM. Empty = all.
    pub kinds: Vec<String>,
}

fn select<'a>(a: &'a Archive, f: &FilterDto) -> Vec<&'a ViewEvent> {
    let filter = Filter {
        from_us: f.from_ms.map(|x| x * 1000),
        to_us: f.to_ms.map(|x| x * 1000),
        app: f.app.clone().filter(|s| !s.trim().is_empty()),
        text: f.text.clone().filter(|s| !s.trim().is_empty()),
        session: None,
    };
    a.select(&filter).into_iter().filter(|e| f.kinds.is_empty() || f.kinds.iter().any(|k| k == e.kind.label())).collect()
}

#[derive(Serialize)]
pub struct EventRow {
    pub seq: u64,
    pub ts_ms: i64,
    pub kind: &'static str,
    pub app: String,
    pub exe_path: String,
    pub title: String,
    pub detail: String,
    pub window_class: String,
    pub pid: u32,
    pub session: String,
    pub focus_ms: i64,
}

#[derive(Serialize)]
pub struct ExploreOut {
    pub total: usize,
    pub rows: Vec<EventRow>,
}

#[tauri::command]
pub async fn explore(state: State<'_, AppState>, filter: FilterDto, offset: usize, limit: usize, newest_first: bool) -> Res<ExploreOut> {
    let s = state.session()?;
    let mut ev = select(&s.archive, &filter);
    let total = ev.len();
    if newest_first {
        ev.reverse();
    }
    let rows = ev
        .into_iter()
        .skip(offset)
        .take(limit.min(500))
        .map(|e| EventRow {
            seq: e.seq,
            ts_ms: e.ts_us / 1000,
            kind: e.kind.label(),
            app: e.app.clone(),
            exe_path: e.exe_path.clone(),
            title: e.title.clone(),
            detail: e.detail.clone(),
            window_class: e.window_class.clone(),
            pid: e.pid,
            session: format!("{:016x}", e.session_id),
            focus_ms: (e.focus_us / 1000) as i64,
        })
        .collect();
    Ok(ExploreOut { total, rows })
}

/// Writes the matching events as PLAINTEXT to `path` (the save dialog already asked about overwriting).
#[tauri::command]
pub async fn export_events(state: State<'_, AppState>, filter: FilterDto, path: String, format: String) -> Res<usize> {
    let s = state.session()?;
    let off = state.offset();
    tauri::async_runtime::spawn_blocking(move || -> Res<usize> {
        let ev = select(&s.archive, &filter);
        let f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path).map_err(|e| format!("cannot write {path}: {e}"))?;
        let mut w = std::io::BufWriter::new(f);
        let r = if format == "json" { export_json(&ev, off, &mut w) } else { export_csv(&ev, off, &mut w) };
        r.and_then(|_| std::io::Write::flush(&mut w)).map_err(|e| e.to_string())?;
        Ok(ev.len())
    })
    .await
    .map_err(blocking_err)?
}

// ------------------------------------------------------------------------------------------------ recorder

#[derive(Serialize)]
pub struct CollectorInfo {
    pub running: bool,
    /// recording | paused | locked | asleep | disconnected | stopped | unknown
    pub state: String,
    pub live: Option<LiveStatus>,
    pub tray_exe: String,
    pub tray_exe_found: bool,
    pub stats: ArchiveStats,
    pub autostart: bool,
}

#[tauri::command]
pub async fn collector_info(state: State<'_, AppState>) -> Res<CollectorInfo> {
    let running = collector::running();
    let live = live::read(&live::live_path(&state.data_dir));
    let st = if !running {
        "stopped".to_string()
    } else {
        match &live {
            Some(l) if l.state != "stopped" => l.state.clone(),
            _ => "unknown".to_string(),
        }
    };
    let exe = collector::tray_exe();
    Ok(CollectorInfo { running, state: st, live: if running { live } else { None }, tray_exe: exe.display().to_string(), tray_exe_found: exe.exists(), stats: cached_stats(&state), autostart: collector::autostart_enabled() })
}

#[tauri::command]
pub async fn collector_control(state: State<'_, AppState>, action: String) -> Res<String> {
    let dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || collector::control(&action, &dir)).await.map_err(blocking_err)?
}

#[derive(Serialize)]
pub struct CollectionSettings {
    pub settings: Settings,
    pub default_exclusions: Vec<String>,
    pub warning: Option<String>,
}

#[tauri::command]
pub async fn collection_settings_get(state: State<'_, AppState>) -> Res<CollectionSettings> {
    let (settings, warning) = Settings::load(&state.data_dir.join("settings.json"));
    Ok(CollectionSettings { settings, default_exclusions: hermes_tracker::DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect(), warning })
}

/// Saved to the same `settings.json` the tray reads. Exclusions take effect the next time recording starts.
#[tauri::command]
pub async fn collection_settings_set(state: State<'_, AppState>, settings: Settings) -> Res<()> {
    settings.save(&state.data_dir.join("settings.json")).map_err(|e| format!("could not save settings: {e}"))
}

#[tauri::command]
pub async fn autostart_set(state: State<'_, AppState>, on: bool) -> Res<bool> {
    collector::set_autostart(on, &state.data_dir)?;
    Ok(collector::autostart_enabled())
}

#[tauri::command]
pub async fn open_data_folder(state: State<'_, AppState>) -> Res<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe").arg(&state.data_dir).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        Err("Opening folders needs Windows.".into())
    }
}
