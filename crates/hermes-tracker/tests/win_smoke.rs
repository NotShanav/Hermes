//! Real Win32 hooks, real windows. Marked #[ignore]: it needs an interactive desktop that allows
//! SetForegroundWindow (a console test process started by hand usually does, services/CI may not).
//!
//!   cargo test -p hermes-tracker --test win_smoke -- --ignored --nocapture
#![cfg(windows)]
#![allow(unsafe_code)]

use hermes_tracker::win::{self, Handle, TrackerConfig};
use hermes_tracker::Sink;
use hermes_tracker::{decode_body, ActivityEvent, Emit, LostReason};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::w;
use windows::Win32::UI::Accessibility::NotifyWinEvent;
use windows::Win32::UI::WindowsAndMessaging::*;

struct Collect(Arc<Mutex<Vec<Emit>>>);
impl Sink for Collect {
    fn emit(&mut self, e: Emit) {
        self.0.lock().unwrap().push(e);
    }
    fn flush(&mut self) {}
    fn has_pending(&self) -> bool {
        false
    }
}

/// Real Windows raises these events by itself; Wine (tested there) does not for SetForegroundWindow /
/// SetWindowText. Raising them explicitly exercises our hook plumbing in both; on Windows the extra
/// notifications are duplicates that the engine de-duplicates.
unsafe fn raise(event: u32, h: windows::Win32::Foundation::HWND) {
    NotifyWinEvent(event, h, 0, 0);
}

/// A thread that owns windows must keep pumping messages (GetWindowText on a same-process window sends
/// WM_GETTEXT to the owner thread). Real targets are other processes, which pump their own windows.
unsafe fn pump_sleep(ms: u64) {
    let end = std::time::Instant::now() + Duration::from_millis(ms);
    let mut msg = MSG::default();
    while std::time::Instant::now() < end {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn helper(handle: Handle) {
    unsafe {
        let mk = |title: windows::core::PCWSTR| {
            CreateWindowExW(
                WS_EX_APPWINDOW,
                w!("STATIC"),
                title,
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                320,
                200,
                None,
                None,
                None,
                None,
            )
            .expect("create window")
        };
        pump_sleep(500);
        let a = mk(w!("Hermes smoke A"));
        let ok = SetForegroundWindow(a);
        println!(
            "[helper] SetForegroundWindow(A)={:?} GetForegroundWindow={:?} A={:?}",
            ok,
            GetForegroundWindow(),
            a
        );
        raise(0x0003, a);
        pump_sleep(900);
        let b = mk(w!("Hermes smoke B"));
        let _ = SetForegroundWindow(b);
        raise(0x0003, b);
        pump_sleep(900);
        let _ = SetWindowTextW(b, w!("Hermes smoke B renamed"));
        raise(0x800C, b);
        pump_sleep(1800);
        raise(0x8001, b); // EVENT_OBJECT_DESTROY (raised before the window disappears, as Windows does)
        let _ = DestroyWindow(b);
        pump_sleep(900);
        let _ = DestroyWindow(a);
        pump_sleep(500);
    }
    handle.stop();
}

#[test]
#[ignore = "needs an interactive desktop that allows SetForegroundWindow"]
fn real_hooks_report_foreground_title_close_and_session_end() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let cfg = TrackerConfig {
        skip_own_process: false,
        flush_ms: 100,
        ..Default::default()
    };
    win::run(cfg, Box::new(Collect(events.clone())), None, |h| {
        std::thread::spawn(move || helper(h));
    })
    .expect("tracker run");

    let evs: Vec<ActivityEvent> = events
        .lock()
        .unwrap()
        .iter()
        .map(|e| decode_body(&e.body).unwrap().event)
        .collect();
    for e in &evs {
        println!("{e:?}");
    }
    let gained = |t: &str| {
        evs.iter().position(|e| matches!(e, ActivityEvent::FocusGained { window, .. } if window.title.as_deref() == Some(t)))
    };
    assert!(
        matches!(evs.first(), Some(ActivityEvent::SessionStart { .. })),
        "first event must be SessionStart"
    );
    assert!(
        matches!(evs.last(), Some(ActivityEvent::SessionEnd { .. })),
        "last event must be SessionEnd"
    );
    let a = gained("Hermes smoke A").expect("focus on window A reported");
    let b = gained("Hermes smoke B").expect("focus on window B reported");
    assert!(a < b, "A before B");
    // switching A -> B produced a FocusLost(Switched) between them
    assert!(evs[a..b].iter().any(|e| matches!(
        e,
        ActivityEvent::FocusLost {
            reason: LostReason::Switched,
            ..
        }
    )));
    // the process identity was resolved for this very test executable
    if let ActivityEvent::FocusGained { app, .. } = &evs[a] {
        assert_eq!(app.pid, std::process::id());
        assert!(
            app.exe_name
                .as_deref()
                .map(|n| n.to_lowercase().contains("win_smoke"))
                .unwrap_or(false),
            "exe_name = {:?}",
            app.exe_name
        );
        assert!(app.process_start.is_some());
    }
    // title change event (generated by the OS for WM_SETTEXT)
    let renamed = evs.iter().any(|e| matches!(e, ActivityEvent::TitleChanged { title, .. } if title.as_deref() == Some("Hermes smoke B renamed")));
    println!("title change observed: {renamed}");
    assert!(
        renamed,
        "EVENT_OBJECT_NAMECHANGE did not produce a TitleChanged event"
    );
    // destroying the foreground window ends focus for a closed/absent window
    assert!(evs.iter().any(|e| matches!(
        e,
        ActivityEvent::FocusLost {
            reason: LostReason::WindowClosed | LostReason::NoForegroundWindow,
            ..
        }
    )));
}

struct RefusingUi;
impl hermes_tracker::win::Ui for RefusingUi {
    fn attach(&mut self, _hwnd: isize, _c: Handle) -> Result<(), String> {
        Err("icon refused".into())
    }
    fn state_changed(&mut self, _s: hermes_tracker::CollectionState) {}
    fn message(&mut self, _h: isize, _m: u32, _w: usize, _l: isize) -> Option<isize> {
        None
    }
    fn detach(&mut self) {}
}

#[test]
#[ignore = "needs a desktop (creates the hidden tracker window)"]
fn recording_indicator_failure_means_nothing_is_recorded() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let r = win::run(TrackerConfig::default(), Box::new(Collect(events.clone())), Some(Box::new(RefusingUi)), |_| panic!("must not reach on_ready"));
    let err = r.expect_err("must fail closed");
    assert!(err.contains("not recording"), "{err}");
    assert!(events.lock().unwrap().is_empty(), "events were recorded without a visible indicator");
}
