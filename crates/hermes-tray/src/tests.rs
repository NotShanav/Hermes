use crate::icon::*;
use crate::menu::*;
use crate::settings::Settings;
use hermes_tracker::CollectionState as S;

fn input(state: S) -> MenuInput {
    MenuInput { state, summary: "12 events · saved 10:00:00 UTC".into(), warnings: 0, autostart: false, start_on_launch: true, dot: true, viewer_available: false, pause_note: None }
}
fn find(m: &[Item], id: u32) -> &Item {
    m.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("menu item {id} missing"))
}

// ---------------------------------------------------------------- settings
#[test]
fn settings_defaults_roundtrip_and_atomic_save() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sub").join("settings.json");
    let (s, w) = Settings::load(&p);
    assert_eq!((s.clone(), w), (Settings::default(), None));
    assert!(s.start_collecting_on_launch && s.show_indicator_dot && s.default_exclusions);
    let s2 = Settings { show_indicator_dot: false, exclusions: vec!["Secret.exe".into(), "  ".into()], ..s };
    s2.save(&p).unwrap();
    assert!(!p.with_extension("json.tmp").exists(), "temp file renamed away");
    let (s3, w) = Settings::load(&p);
    assert_eq!((s3.clone(), w), (s2, None));
    let ex = s3.effective_exclusions();
    assert!(ex.contains(&"Secret.exe".to_string()) && ex.contains(&"keepass.exe".to_string()));
    assert!(!ex.iter().any(|e| e.trim().is_empty()));
}

#[test]
fn damaged_settings_fall_back_to_defaults_without_overwriting_the_file() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.json");
    std::fs::write(&p, "{ not json").unwrap();
    let (s, w) = Settings::load(&p);
    assert_eq!(s, Settings::default());
    assert!(w.unwrap().contains("not valid"));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json", "user's file untouched");
    // unknown / partial files are tolerated
    std::fs::write(&p, r#"{"show_indicator_dot": false, "future_option": 1}"#).unwrap();
    let (s, w) = Settings::load(&p);
    assert!(w.is_none() && !s.show_indicator_dot && s.start_collecting_on_launch);
}

#[test]
fn default_exclusions_can_be_switched_off_but_never_silently() {
    let s = Settings { default_exclusions: false, exclusions: vec!["a.exe".into()], ..Settings::default() };
    assert_eq!(s.effective_exclusions(), vec!["a.exe".to_string()]);
}

// ---------------------------------------------------------------- icon
fn px(buf: &[u8], size: u32, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let i = ((y * size + x) * 4) as usize;
    (buf[i + 2], buf[i + 1], buf[i], buf[i + 3]) // r,g,b,a (un-premultiply not needed at alpha 255)
}

#[test]
fn recording_icon_is_a_red_disc_and_corners_are_transparent() {
    for size in [16u32, 20, 24, 32, 48] {
        let b = pixels(IconKind::Recording, size);
        assert_eq!(b.len() as u32, size * size * 4);
        assert_eq!(px(&b, size, 0, 0).3, 0, "corner transparent at {size}");
        let (r, g, bl, a) = px(&b, size, size / 2 - 1, (size as f32 * 0.12) as u32 + size / 2 - size / 2);
        let _ = (g, bl);
        assert!(a > 0, "{size}");
        // somewhere in the disc, between ring and edge, the colour is the record red
        let red = (0..size).any(|x| {
            let (r, g, b, a) = px(&b, size, x, size / 2);
            a == 255 && r > 180 && g < 80 && b < 80
        });
        assert!(red, "no solid red pixel at size {size}");
        let _ = r;
    }
}

#[test]
fn every_state_has_a_distinct_look() {
    use std::collections::HashSet;
    let kinds = [IconKind::Recording, IconKind::Paused, IconKind::Inactive, IconKind::Warning];
    let imgs: HashSet<Vec<u8>> = kinds.iter().map(|k| pixels(*k, 32)).collect();
    assert_eq!(imgs.len(), 4);
    // paused is visibly NOT red; warning is amber; inactive is mostly transparent (hollow)
    let paused = pixels(IconKind::Paused, 32);
    assert!(!(0..32).any(|x| {
        let (r, g, b, a) = px(&paused, 32, x, 16);
        a == 255 && r > 180 && g < 80 && b < 80
    }));
    let warn = pixels(IconKind::Warning, 32);
    assert!((0..32).any(|x| {
        let (r, g, b, a) = px(&warn, 32, x, 6);
        a == 255 && r > 200 && g > 140 && b < 60
    }));
    let opaque = |b: &[u8]| b.chunks(4).filter(|p| p[3] > 128).count();
    assert!(opaque(&pixels(IconKind::Inactive, 32)) < opaque(&pixels(IconKind::Recording, 32)) / 2);
}

#[test]
fn icon_choice_follows_state_and_warnings() {
    assert_eq!(kind_for(S::Recording, false), IconKind::Recording);
    assert_eq!(kind_for(S::Recording, true), IconKind::Warning);
    assert_eq!(kind_for(S::Paused, true), IconKind::Paused, "paused is paused even if a warning exists");
    for s in [S::Locked, S::Asleep, S::Disconnected, S::Stopped] {
        assert_eq!(kind_for(s, false), IconKind::Inactive);
    }
}

// ---------------------------------------------------------------- menu
#[test]
fn recording_menu_says_recording_and_offers_pause() {
    let m = build(&input(S::Recording));
    assert!(m[0].label.contains("RECORDING") && !m[0].enabled);
    let pr = find(&m, ids::PAUSE_RESUME);
    assert_eq!((pr.label.as_str(), pr.enabled), ("Pause collection", true));
    assert!(find(&m, ids::PAUSE_15).enabled && find(&m, ids::PAUSE_60).enabled);
}

#[test]
fn paused_menu_offers_resume_and_never_claims_recording() {
    let mut i = input(S::Paused);
    i.pause_note = Some("auto-resumes at 15:20".into());
    let m = build(&i);
    assert!(m[0].label.contains("PAUSED") && m[0].label.contains("auto-resumes at 15:20"));
    assert!(!m.iter().any(|x| x.label.contains("RECORDING")));
    let pr = find(&m, ids::PAUSE_RESUME);
    assert_eq!((pr.label.as_str(), pr.enabled), ("Resume collection", true));
    assert!(!find(&m, ids::PAUSE_15).enabled && !find(&m, ids::PAUSE_60).enabled);
}

#[test]
fn system_states_are_described_and_pause_is_unavailable() {
    for s in [S::Locked, S::Asleep, S::Disconnected] {
        let m = build(&input(s));
        assert!(m[0].label.contains("Not recording"), "{:?}", m[0]);
        assert!(!find(&m, ids::PAUSE_RESUME).enabled);
        assert!(find(&m, ids::EXIT).enabled && find(&m, ids::STATUS).enabled);
    }
}

#[test]
fn warnings_checks_viewer_and_exit_are_reflected() {
    let mut i = input(S::Recording);
    i.warnings = 2;
    i.autostart = true;
    i.start_on_launch = false;
    i.dot = false;
    let m = build(&i);
    assert!(m.iter().any(|x| x.label.contains("2 warning(s)")));
    assert!(find(&m, ids::AUTOSTART).checked && !find(&m, ids::START_ON_LAUNCH).checked && !find(&m, ids::DOT).checked);
    let v = find(&m, ids::OPEN_VIEWER);
    assert!(!v.enabled && v.label.contains("not installed"));
    i.viewer_available = true;
    assert!(find(&build(&i), ids::OPEN_VIEWER).enabled);
    let last = m.last().unwrap();
    assert_eq!(last.id, ids::EXIT);
    assert!(last.label.contains("stops recording"));
}

#[test]
fn menu_ids_are_unique_and_all_actions_exist() {
    let m = build(&input(S::Recording));
    let ids_: Vec<u32> = m.iter().filter(|i| i.id != 0).map(|i| i.id).collect();
    let set: std::collections::HashSet<_> = ids_.iter().collect();
    assert_eq!(set.len(), ids_.len());
    for id in [ids::PAUSE_RESUME, ids::PAUSE_15, ids::PAUSE_60, ids::STATUS, ids::OPEN_VIEWER, ids::AUTOSTART, ids::START_ON_LAUNCH, ids::DOT, ids::OPEN_FOLDER, ids::OPEN_SETTINGS, ids::EXIT] {
        assert!(ids_.contains(&id), "{id}");
    }
}

#[test]
fn tooltip_fits_the_shell_limit_and_names_the_state() {
    let t = tooltip(S::Recording, &"x".repeat(500));
    assert!(t.chars().count() <= 127);
    assert!(t.starts_with("Hermes: Recording"));
    assert!(tooltip(S::Paused, "").contains("Paused"));
}

#[test]
fn autostart_command_quotes_paths_with_spaces_and_has_no_hidden_flags() {
    let c = autostart_command("C:\\Program Files\\Hermes\\hermes-tray.exe", "C:\\Users\\A B\\AppData\\Local\\Hermes");
    assert_eq!(c, "\"C:\\Program Files\\Hermes\\hermes-tray.exe\" --data-dir \"C:\\Users\\A B\\AppData\\Local\\Hermes\"");
    assert!(!c.contains("hidden") && !c.contains("silent") && !c.contains("quiet"));
}

#[test]
fn summary_line_is_compact_and_honest_about_drops() {
    assert_eq!(summary(0, 0, 0), "0 events · nothing saved yet");
    assert_eq!(summary(5, 0, 3_723_000_000), "5 events · saved 01:02:03 UTC");
    assert!(summary(5, 2, 0).contains("2 dropped"));
}
