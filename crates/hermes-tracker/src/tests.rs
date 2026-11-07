use crate::engine::*;
use crate::model::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Default)]
struct World {
    wall: i64,
    mono: u64,
    next_sid: u64,
    fg: Option<u64>,
    windows: HashMap<u64, Focus>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<World>>);

impl Platform for Fake {
    fn wall_us(&self) -> i64 {
        self.0.borrow().wall
    }
    fn mono_us(&self) -> u64 {
        self.0.borrow().mono
    }
    fn new_session_id(&self) -> u64 {
        let mut w = self.0.borrow_mut();
        w.next_sid += 1;
        1000 + w.next_sid
    }
    fn foreground(&self) -> Option<u64> {
        self.0.borrow().fg
    }
    fn describe(&self, hwnd: u64) -> Result<Focus, DescribeError> {
        self.0
            .borrow()
            .windows
            .get(&hwnd)
            .cloned()
            .ok_or(DescribeError::WindowGone)
    }
}

impl Fake {
    fn advance(&self, us: u64) {
        let mut w = self.0.borrow_mut();
        w.mono += us;
        w.wall += us as i64;
    }
    fn add(&self, hwnd: u64, pid: u32, exe: &str, title: &str) {
        self.0
            .borrow_mut()
            .windows
            .insert(hwnd, focus(hwnd, pid, exe, title));
    }
    fn set_title(&self, hwnd: u64, title: &str) {
        let mut w = self.0.borrow_mut();
        let f = w.windows.get_mut(&hwnd).unwrap();
        f.window.title = Some(title.into());
        f.window.title_status = TitleStatus::Text;
    }
    fn remove(&self, hwnd: u64) {
        self.0.borrow_mut().windows.remove(&hwnd);
    }
    fn set_fg(&self, h: Option<u64>) {
        self.0.borrow_mut().fg = h;
    }
}

fn focus(hwnd: u64, pid: u32, exe: &str, title: &str) -> Focus {
    Focus {
        app: AppIdentity {
            pid,
            process_start: Some(5000 + pid as u64),
            exe_path: Some(format!("C:\\Program Files\\{exe}")),
            exe_name: Some(exe.into()),
            status: IdentityStatus::Resolved,
            host_exe: None,
        },
        window: WindowInfo {
            hwnd,
            class: Some("Cls".into()),
            title: Some(title.into()),
            title_status: TitleStatus::Text,
            title_truncated: false,
        },
    }
}

fn engine() -> (Engine<Fake>, Fake) {
    let f = Fake::default();
    f.advance(1_000_000);
    (Engine::new(f.clone(), Config::default()), f)
}

fn evs(out: &[Emit]) -> Vec<ActivityEvent> {
    out.iter()
        .map(|e| decode_body(&e.body).unwrap().event)
        .collect()
}

fn names(out: &[Emit]) -> Vec<&'static str> {
    evs(out)
        .iter()
        .map(|e| match e {
            ActivityEvent::SessionStart { .. } => "start",
            ActivityEvent::SessionEnd { .. } => "end",
            ActivityEvent::FocusGained { .. } => "gain",
            ActivityEvent::FocusLost { .. } => "lost",
            ActivityEvent::TitleChanged { .. } => "title",
        })
        .collect()
}

#[test]
fn start_opens_a_session_and_reports_the_current_foreground_window() {
    let (mut e, f) = engine();
    f.add(1, 10, "notepad.exe", "Untitled - Notepad");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    assert_eq!(names(&out), ["start", "gain"]);
    assert!(out.iter().all(|o| o.session_id == 1001));
    match &evs(&out)[1] {
        ActivityEvent::FocusGained {
            app,
            window,
            from_hwnd,
        } => {
            assert_eq!(app.exe_name.as_deref(), Some("notepad.exe"));
            assert_eq!(
                (window.hwnd, window.title.as_deref(), *from_hwnd),
                (1, Some("Untitled - Notepad"), None)
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(out[0].kind, kinds::SESSION_START);
    assert_eq!(out[1].kind, kinds::FOCUS_GAINED);
}

#[test]
fn start_without_a_foreground_window_emits_only_the_session_start() {
    let (mut e, _f) = engine();
    let mut out = vec![];
    e.start(&mut out);
    assert_eq!(names(&out), ["start"]);
}

#[test]
fn switching_emits_lost_then_gained_with_monotonic_duration_and_previous_window() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.add(2, 20, "b.exe", "B");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    f.advance(2_500_000);
    e.handle(Observation::Foreground(Some(2)), &mut out);
    assert_eq!(names(&out), ["lost", "gain"]);
    match &evs(&out)[0] {
        ActivityEvent::FocusLost {
            reason,
            hwnd,
            pid,
            focus_us,
        } => {
            assert_eq!(
                (*reason, *hwnd, *pid, *focus_us),
                (LostReason::Switched, 1, 10, 2_500_000)
            );
        }
        o => panic!("{o:?}"),
    }
    match &evs(&out)[1] {
        ActivityEvent::FocusGained { from_hwnd, .. } => assert_eq!(*from_hwnd, Some(1)),
        o => panic!("{o:?}"),
    }
}

#[test]
fn duplicate_foreground_notifications_are_ignored() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    for _ in 0..5 {
        e.handle(Observation::Foreground(Some(1)), &mut out);
    }
    assert!(out.is_empty());
}

#[test]
fn multiple_windows_of_one_application_are_distinct_foreground_targets() {
    let (mut e, f) = engine();
    f.add(1, 10, "chrome.exe", "Inbox - Mail");
    f.add(2, 10, "chrome.exe", "Docs");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    e.handle(Observation::Foreground(Some(2)), &mut out);
    assert_eq!(names(&out), ["lost", "gain"]);
    match &evs(&out)[1] {
        ActivityEvent::FocusGained { app, window, .. } => {
            assert_eq!((app.pid, window.hwnd), (10, 2))
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn reused_pid_or_hwnd_is_not_mistaken_for_the_same_process() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    // same hwnd and pid, but a different process instance (new creation time)
    f.0.borrow_mut()
        .windows
        .get_mut(&1)
        .unwrap()
        .app
        .process_start = Some(999_999);
    e.handle(Observation::Foreground(Some(1)), &mut out);
    assert_eq!(names(&out), ["lost", "gain"]);
}

#[test]
fn title_changes_are_coalesced_rate_limited_and_deduplicated() {
    let (mut e, f) = engine();
    f.add(1, 10, "browser.exe", "Page 0");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();

    // a burst of changes -> exactly one event carrying the final title, after the coalescing delay
    for i in 1..=20 {
        f.set_title(1, &format!("Page {i}"));
        e.handle(Observation::TitleHint(1), &mut out);
        f.advance(10_000);
    }
    assert!(out.is_empty(), "nothing reported before the title is quiet");
    let due = e.next_deadline().expect("deadline armed");
    assert!(due > f.mono_us());
    f.advance(due - f.mono_us());
    e.tick(&mut out);
    assert_eq!(names(&out), ["title"]);
    match &evs(&out)[0] {
        ActivityEvent::TitleChanged { title, .. } => assert_eq!(title.as_deref(), Some("Page 20")),
        o => panic!("{o:?}"),
    }
    out.clear();

    // hint with unchanged title: no event
    e.handle(Observation::TitleHint(1), &mut out);
    f.advance(e.next_deadline().unwrap() - f.mono_us());
    e.tick(&mut out);
    assert!(out.is_empty());

    // title animating continuously: never more often than the minimum interval
    let mut emitted_at = vec![];
    for step in 0..40 {
        f.set_title(1, &format!("tick {step}"));
        e.handle(Observation::TitleHint(1), &mut out);
        f.advance(100_000);
        e.tick(&mut out);
        if !out.is_empty() {
            emitted_at.push(f.mono_us());
            out.clear();
        }
    }
    assert!(emitted_at.len() >= 3);
    for w in emitted_at.windows(2) {
        assert!(
            w[1] - w[0] >= 1_000_000,
            "rate limit violated: {:?}",
            emitted_at
        );
    }
    // hints for other (background) windows are ignored
    e.handle(Observation::TitleHint(99), &mut out);
    assert!(e.next_deadline().is_none() || out.is_empty());
}

#[test]
fn closed_window_is_reported_and_focus_resyncs() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.add(2, 20, "b.exe", "B");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    // window 1 is destroyed and the OS already moved focus to window 2
    f.remove(1);
    f.set_fg(Some(2));
    e.handle(Observation::Destroyed(1), &mut out);
    assert_eq!(names(&out), ["lost", "gain"]);
    match &evs(&out)[0] {
        ActivityEvent::FocusLost { reason, .. } => assert_eq!(*reason, LostReason::WindowClosed),
        o => panic!("{o:?}"),
    }
}

#[test]
fn window_that_vanishes_before_it_can_be_described_is_handled_without_error() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    // notification arrives for a window that is already gone (fast-closing dialog / splash screen)
    e.handle(Observation::Foreground(Some(77)), &mut out);
    assert_eq!(names(&out), ["lost"]);
    match &evs(&out)[0] {
        ActivityEvent::FocusLost { reason, .. } => assert_eq!(*reason, LostReason::WindowClosed),
        o => panic!("{o:?}"),
    }
    out.clear();
    // and one that never had focus in our records produces nothing at all
    e.handle(Observation::Foreground(Some(78)), &mut out);
    assert!(out.is_empty());
}

#[test]
fn no_foreground_window_e_g_secure_desktop_is_recorded_honestly() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    e.handle(Observation::Foreground(None), &mut out);
    assert_eq!(names(&out), ["lost"]);
    match &evs(&out)[0] {
        ActivityEvent::FocusLost { reason, .. } => {
            assert_eq!(*reason, LostReason::NoForegroundWindow)
        }
        o => panic!("{o:?}"),
    }
    out.clear();
    e.handle(Observation::Foreground(None), &mut out); // repeated: nothing
    assert!(out.is_empty());
    e.handle(Observation::Foreground(Some(1)), &mut out);
    assert_eq!(names(&out), ["gain"]);
}

#[test]
fn lock_unlock_defines_session_boundaries_and_ignores_events_while_locked() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.add(2, 20, "b.exe", "B");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    let first_session = e.session_id().unwrap();
    out.clear();
    e.handle(Observation::Lock, &mut out);
    assert_eq!(names(&out), ["lost", "end"]);
    assert!(matches!(
        &evs(&out)[1],
        ActivityEvent::SessionEnd {
            reason: EndReason::Lock
        }
    ));
    assert!(e.session_id().is_none());
    out.clear();
    e.handle(Observation::Foreground(Some(2)), &mut out); // lock screen / background chatter
    e.handle(Observation::TitleHint(2), &mut out);
    assert!(out.is_empty());
    f.set_fg(Some(2));
    e.handle(Observation::Unlock, &mut out);
    assert_eq!(names(&out), ["start", "gain"]);
    assert!(matches!(
        &evs(&out)[0],
        ActivityEvent::SessionStart {
            reason: StartReason::Unlock,
            ..
        }
    ));
    assert_ne!(
        e.session_id().unwrap(),
        first_session,
        "new session id after unlock"
    );
}

#[test]
fn pause_lock_sleep_combinations_resume_only_when_every_cause_is_gone() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    e.handle(Observation::Pause, &mut out);
    e.handle(Observation::Lock, &mut out);
    e.handle(Observation::Suspend, &mut out);
    assert_eq!(
        names(&out),
        ["lost", "end"],
        "only the first cause ends the session"
    );
    assert!(matches!(
        &evs(&out)[1],
        ActivityEvent::SessionEnd {
            reason: EndReason::Pause
        }
    ));
    out.clear();
    e.handle(Observation::Unlock, &mut out);
    e.handle(Observation::Resume, &mut out);
    assert!(out.is_empty(), "still paused by the user");
    assert!(!e.is_active());
    e.handle(Observation::ResumeCollection, &mut out);
    assert_eq!(names(&out), ["start", "gain"]);
    assert!(matches!(
        &evs(&out)[0],
        ActivityEvent::SessionStart {
            reason: StartReason::ResumeCollection,
            ..
        }
    ));
    // spurious unlock while active changes nothing
    out.clear();
    e.handle(Observation::Unlock, &mut out);
    assert!(out.is_empty());
}

#[test]
fn sleep_and_wake_and_logoff_and_disconnect_map_to_reasons() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    for (down, up, end, start) in [
        (
            Observation::Suspend,
            Observation::Resume,
            EndReason::Sleep,
            StartReason::Resume,
        ),
        (
            Observation::Disconnect,
            Observation::Reconnect,
            EndReason::Disconnect,
            StartReason::Reconnect,
        ),
    ] {
        out.clear();
        e.handle(down, &mut out);
        assert!(matches!(&evs(&out)[1], ActivityEvent::SessionEnd { reason } if *reason == end));
        out.clear();
        e.handle(up, &mut out);
        assert!(
            matches!(&evs(&out)[0], ActivityEvent::SessionStart { reason, .. } if *reason == start)
        );
    }
    out.clear();
    e.handle(Observation::Logoff, &mut out);
    assert!(matches!(
        &evs(&out)[1],
        ActivityEvent::SessionEnd {
            reason: EndReason::Logoff
        }
    ));
}

#[test]
fn stop_closes_the_session_and_nothing_is_emitted_afterwards() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.add(2, 20, "b.exe", "B");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    e.handle(Observation::Stop, &mut out);
    assert_eq!(names(&out), ["lost", "end"]);
    assert!(matches!(
        &evs(&out)[1],
        ActivityEvent::SessionEnd {
            reason: EndReason::CollectorStop
        }
    ));
    out.clear();
    e.handle(Observation::Foreground(Some(2)), &mut out);
    e.handle(Observation::Unlock, &mut out);
    e.handle(Observation::ResumeCollection, &mut out);
    assert!(out.is_empty() && e.is_stopped());
}

#[test]
fn excluded_applications_leave_no_identifying_information() {
    let f = Fake::default();
    let mut cfg = Config::default();
    cfg.exclusions.push("Secret-App.EXE".into()); // case-insensitive
    let mut e = Engine::new(f.clone(), cfg);
    f.add(1, 10, "KeePassXC.exe", "Passwords - KeePassXC");
    f.add(2, 11, "secret-app.exe", "Project Phoenix");
    f.add(3, 12, "notepad.exe", "ok");
    let mut out = vec![];
    for (h, _) in [(1u64, ()), (2, ()), (3, ())] {
        f.set_fg(Some(h));
        if h == 1 {
            e.start(&mut out);
        } else {
            e.handle(Observation::Foreground(Some(h)), &mut out);
        }
    }
    let raw = String::from_utf8(out.iter().flat_map(|o| o.body.clone()).collect()).unwrap();
    for leaked in [
        "KeePass",
        "Passwords",
        "secret-app",
        "Secret",
        "Phoenix",
        "Program Files\\\\KeePass",
    ] {
        assert!(!raw.contains(leaked), "leaked {leaked:?} in {raw}");
    }
    assert!(
        raw.contains("notepad.exe"),
        "non-excluded app is still recorded"
    );
    let gains: Vec<_> = evs(&out)
        .into_iter()
        .filter_map(|e| {
            if let ActivityEvent::FocusGained { app, window, .. } = e {
                Some((app, window))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        gains.len(),
        3,
        "timeline has no holes: excluded focus is still marked"
    );
    assert_eq!(gains[0].0.status, IdentityStatus::Excluded);
    assert_eq!(gains[0].1.title_status, TitleStatus::Redacted);
    assert_eq!(gains[2].0.status, IdentityStatus::Resolved);
}

#[test]
fn title_changes_of_excluded_apps_are_not_reported_and_uwp_host_exclusion_applies() {
    let (mut e, f) = engine();
    f.add(1, 10, "keepass.exe", "Vault A");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    f.set_title(1, "Vault B - secret entry");
    e.handle(Observation::TitleHint(1), &mut out);
    f.advance(e.next_deadline().unwrap() - f.mono_us());
    e.tick(&mut out);
    assert!(
        out.is_empty(),
        "redacted title never changes, so nothing is emitted"
    );
    // a packaged app resolved behind a host process that is excluded
    let mut ff = focus(5, 50, "WindowsCredentialApp.exe", "Sign in");
    ff.app.host_exe = Some("LogonUI.exe".into());
    f.0.borrow_mut().windows.insert(5, ff);
    f.set_fg(Some(5));
    e.handle(Observation::Foreground(Some(5)), &mut out);
    let raw = String::from_utf8(out.iter().flat_map(|o| o.body.clone()).collect()).unwrap();
    assert!(!raw.contains("Sign in") && !raw.contains("WindowsCredentialApp"));
}

#[test]
fn inaccessible_processes_are_recorded_as_such_not_invented() {
    let (mut e, f) = engine();
    let mut w = focus(1, 4, "System", "Task Manager");
    w.app.exe_name = None;
    w.app.exe_path = None;
    w.app.status = IdentityStatus::AccessDenied;
    f.0.borrow_mut().windows.insert(1, w);
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    match &evs(&out)[1] {
        ActivityEvent::FocusGained { app, window, .. } => {
            assert_eq!(
                (app.status, app.exe_name.clone()),
                (IdentityStatus::AccessDenied, None)
            );
            assert_eq!(
                window.title.as_deref(),
                Some("Task Manager"),
                "window title may still be readable"
            );
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn very_long_titles_are_truncated_and_flagged() {
    let f = Fake::default();
    let mut cfg = Config::default();
    cfg.max_title_chars = 10;
    let mut e = Engine::new(f.clone(), cfg);
    f.add(1, 10, "a.exe", "ééééééééééééééééééééééé");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    match &evs(&out)[1] {
        ActivityEvent::FocusGained { window, .. } => {
            assert_eq!(window.title.as_deref().unwrap().chars().count(), 10);
            assert!(window.title_truncated);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn timestamps_never_go_backwards_and_every_event_carries_session_and_mono_time() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.add(2, 20, "b.exe", "B");
    f.set_fg(Some(1));
    let mut out = vec![];
    e.start(&mut out);
    for (i, obs) in [
        Observation::Foreground(Some(2)),
        Observation::Foreground(Some(1)),
        Observation::Lock,
        Observation::Unlock,
        Observation::Foreground(Some(2)),
    ]
    .into_iter()
    .enumerate()
    {
        f.advance(1000 * (i as u64 + 1));
        if let Observation::Foreground(h) = obs {
            f.set_fg(h);
        }
        e.handle(obs, &mut out);
    }
    let bodies: Vec<_> = out.iter().map(|o| decode_body(&o.body).unwrap()).collect();
    assert!(out.windows(2).all(|w| w[0].ts_us <= w[1].ts_us));
    assert!(bodies.windows(2).all(|w| w[0].mono_us <= w[1].mono_us));
    assert!(out.iter().all(|o| o.session_id != 0));
}

#[test]
fn body_format_is_stable_json() {
    // Golden test: the stored body format is a compatibility surface (viewer, future migrations).
    let ev = ActivityEvent::FocusGained {
        app: AppIdentity {
            pid: 7,
            process_start: Some(1),
            exe_path: Some("C:\\a.exe".into()),
            exe_name: Some("a.exe".into()),
            status: IdentityStatus::Resolved,
            host_exe: None,
        },
        window: WindowInfo {
            hwnd: 9,
            class: Some("C".into()),
            title: Some("T".into()),
            title_status: TitleStatus::Text,
            title_truncated: false,
        },
        from_hwnd: None,
    };
    let json = String::from_utf8(encode_body(42, &ev)).unwrap();
    assert_eq!(
        json,
        r#"{"v":1,"mono_us":42,"t":"focus_gained","app":{"pid":7,"process_start":1,"exe_path":"C:\\a.exe","exe_name":"a.exe","status":"resolved"},"window":{"hwnd":9,"class":"C","title":"T","title_status":"text"}}"#
    );
    assert_eq!(
        decode_body(json.as_bytes()).unwrap(),
        EventBody {
            v: 1,
            mono_us: 42,
            event: ev
        }
    );
    assert!(decode_body(b"{\"v\":1}").is_err());
}

#[test]
fn kinds_are_unique_and_outside_the_storage_reserved_range() {
    let all = [
        kinds::SESSION_START,
        kinds::SESSION_END,
        kinds::FOCUS_GAINED,
        kinds::FOCUS_LOST,
        kinds::TITLE_CHANGED,
    ];
    let set: std::collections::HashSet<_> = all.iter().collect();
    assert_eq!(set.len(), all.len());
    assert!(
        all.iter().all(|k| *k > 255),
        "0..=255 is reserved for the storage layer"
    );
}

#[test]
fn destroyed_window_still_reported_as_foreground_does_not_create_a_ghost_focus() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.add(2, 20, "b.exe", "B");
    f.set_fg(Some(2));
    let mut out = vec![];
    e.start(&mut out);
    out.clear();
    // B is being destroyed, but for a moment the OS still says B is the foreground window
    // (and the window may even still be describable).
    e.handle(Observation::Destroyed(2), &mut out);
    assert_eq!(
        names(&out),
        ["lost"],
        "no ghost FocusGained for the dying window"
    );
    out.clear();
    e.handle(Observation::Foreground(Some(2)), &mut out);
    assert!(out.is_empty());
    // once the OS moves on, tracking resumes normally
    f.set_fg(Some(1));
    e.handle(Observation::Foreground(Some(1)), &mut out);
    assert_eq!(names(&out), ["gain"]);
}

#[test]
fn collection_state_reflects_the_most_relevant_cause() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    let mut out = vec![];
    assert_eq!(e.state(), CollectionState::Recording);
    e.start(&mut out);
    e.handle(Observation::Suspend, &mut out);
    assert_eq!(e.state(), CollectionState::Asleep);
    e.handle(Observation::Lock, &mut out);
    assert_eq!(e.state(), CollectionState::Locked, "lock outranks sleep");
    e.handle(Observation::Pause, &mut out);
    assert_eq!(e.state(), CollectionState::Paused, "user pause outranks everything");
    e.handle(Observation::ResumeCollection, &mut out);
    assert_eq!(e.state(), CollectionState::Locked);
    e.handle(Observation::Unlock, &mut out);
    e.handle(Observation::Resume, &mut out);
    assert_eq!(e.state(), CollectionState::Recording);
    e.handle(Observation::Stop, &mut out);
    assert_eq!(e.state(), CollectionState::Stopped);
}

#[test]
fn starting_paused_records_nothing_until_resumed() {
    let (mut e, f) = engine();
    f.add(1, 10, "a.exe", "A");
    f.set_fg(Some(1));
    e.start_paused();
    let mut out = vec![];
    assert_eq!(e.state(), CollectionState::Paused);
    e.handle(Observation::Foreground(Some(1)), &mut out);
    e.handle(Observation::TitleHint(1), &mut out);
    assert!(out.is_empty() && e.session_id().is_none(), "nothing read, nothing recorded while paused");
    e.handle(Observation::ResumeCollection, &mut out);
    assert_eq!(names(&out), ["start", "gain"]);
}

#[test]
fn only_observations_that_end_recording_demand_an_immediate_durable_flush() {
    for o in [Observation::Lock, Observation::Suspend, Observation::Logoff, Observation::Disconnect, Observation::Pause, Observation::Stop] {
        assert!(needs_durable_flush(&o), "{o:?}");
    }
    for o in [Observation::Foreground(Some(1)), Observation::TitleHint(1), Observation::Destroyed(1), Observation::Unlock, Observation::Resume, Observation::Reconnect, Observation::ResumeCollection] {
        assert!(!needs_durable_flush(&o), "{o:?}");
    }
}
