//! Windows backend. Event-driven, single message-loop thread, no polling, no injection.
//!
//! * Foreground / title / destroy: `SetWinEventHook(..., WINEVENT_OUTOFCONTEXT)`. Out-of-context hooks
//!   are delivered as messages to *this* thread; nothing is loaded into other processes.
//! * Lock/unlock/logoff/disconnect: `WTSRegisterSessionNotification`.
//! * Sleep/resume: `WM_POWERBROADCAST` on a hidden top-level window.
//! * Process identity: `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `QueryFullProcessImageNameW`.
//! * Wall clock `GetSystemTimePreciseAsFileTime` (100 ns), monotonic clock `Instant` (QPC).
//!
//! No keyboard/mouse hooks, no screen capture, no text. All `unsafe` in the tracker lives here.
//! NOT yet exercised on real Windows hardware by the author: see docs/STATUS.md.

#![allow(unsafe_code)]

use crate::engine::{needs_durable_flush, CollectionState, Config, DescribeError, Emit, Engine, Observation, Platform};
use crate::sink::Sink;
use crate::model::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;
use windows::core::{w, BOOL, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, ERROR_CLASS_ALREADY_EXISTS, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER,
    FILETIME, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WIN32_ERROR, WPARAM,
};
use windows::Win32::System::Console::{
    SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

const EVENT_SYSTEM_FOREGROUND_: u32 = 0x0003;
const EVENT_OBJECT_DESTROY_: u32 = 0x8001;
const EVENT_OBJECT_NAMECHANGE_: u32 = 0x800C;
const WINEVENT_OUTOFCONTEXT_: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS_: u32 = 0x0002;

const WM_NUDGE: u32 = WM_APP + 10;
const WM_CTL_PAUSE: u32 = WM_APP + 1;
const WM_CTL_RESUME: u32 = WM_APP + 2;
const WM_CTL_STOP: u32 = WM_APP + 3;
const TIMER_TITLE: usize = 1;
const TIMER_FLUSH: usize = 2;
const QUEUE_CAP: usize = 512;

// ---------------------------------------------------------------------------------------------
// Platform implementation

pub struct WinPlatform {
    start: Instant,
}

impl WinPlatform {
    pub fn new() -> Self {
        WinPlatform {
            start: Instant::now(),
        }
    }
}

impl Default for WinPlatform {
    fn default() -> Self {
        Self::new()
    }
}

fn filetime_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

fn hwnd_of(h: u64) -> HWND {
    HWND(h as usize as *mut c_void)
}

fn hwnd_u64(h: HWND) -> u64 {
    h.0 as usize as u64
}

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn process_identity(pid: u32) -> AppIdentity {
    let mut app = AppIdentity {
        pid,
        process_start: None,
        exe_path: None,
        exe_name: None,
        status: IdentityStatus::Unavailable,
        host_exe: None,
    };
    unsafe {
        let h = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => OwnedHandle(h),
            Err(e) => {
                let code = e.code();
                app.status = if code == windows::core::HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
                    IdentityStatus::AccessDenied
                } else if code == windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) {
                    IdentityStatus::ProcessGone
                } else {
                    IdentityStatus::Unavailable
                };
                return app;
            }
        };
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
            .is_ok()
        {
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            app.exe_name = path.rsplit(['\\', '/']).next().map(|s| s.to_string());
            app.exe_path = Some(path);
            app.status = IdentityStatus::Resolved;
        }
        let (mut c, mut x, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        if GetProcessTimes(h.0, &mut c, &mut x, &mut k, &mut u).is_ok() {
            app.process_start = Some(filetime_u64(c));
        }
    }
    app
}

struct ChildSearch {
    host_pid: u32,
    found: u32,
}

unsafe extern "system" fn enum_child(h: HWND, lp: LPARAM) -> BOOL {
    let st = &mut *(lp.0 as *mut ChildSearch);
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid != 0 && pid != st.host_pid {
        st.found = pid;
        return BOOL(0); // stop enumeration
    }
    BOOL(1)
}

fn window_title(h: HWND) -> (Option<String>, TitleStatus) {
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let n = GetWindowTextLengthW(h);
        if n <= 0 {
            return if GetLastError() == WIN32_ERROR(0) {
                (None, TitleStatus::Empty)
            } else {
                (None, TitleStatus::Unavailable)
            };
        }
        let mut buf = vec![0u16; (n as usize + 1).min(65536)];
        let got = GetWindowTextW(h, &mut buf);
        if got <= 0 {
            return (None, TitleStatus::Unavailable);
        }
        (
            Some(String::from_utf16_lossy(&buf[..got as usize])),
            TitleStatus::Text,
        )
    }
}

fn window_class(h: HWND) -> Option<String> {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    if n > 0 {
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    } else {
        None
    }
}

impl Platform for WinPlatform {
    fn wall_us(&self) -> i64 {
        // FILETIME is 100 ns ticks since 1601-01-01; Unix epoch offset is 11644473600 s.
        let ft = filetime_u64(unsafe { GetSystemTimePreciseAsFileTime() });
        (ft.saturating_sub(116_444_736_000_000_000) / 10) as i64
    }

    fn mono_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    fn new_session_id(&self) -> u64 {
        loop {
            let mut b = [0u8; 8];
            getrandom::fill(&mut b).expect("OS RNG");
            let v = u64::from_le_bytes(b);
            if v != 0 {
                return v;
            }
        }
    }

    fn foreground(&self) -> Option<u64> {
        let h = unsafe { GetForegroundWindow() };
        if h.0.is_null() {
            None
        } else {
            Some(hwnd_u64(h))
        }
    }

    fn describe(&self, hwnd: u64) -> Result<Focus, DescribeError> {
        let h = hwnd_of(hwnd);
        unsafe {
            if !IsWindow(Some(h)).as_bool() {
                return Err(DescribeError::WindowGone);
            }
            let mut pid = 0u32;
            if GetWindowThreadProcessId(h, Some(&mut pid)) == 0 || pid == 0 {
                return Err(DescribeError::WindowGone);
            }
            let mut app = process_identity(pid);
            // Packaged (UWP) apps are hosted in ApplicationFrameHost.exe; the real app owns a child window.
            if app
                .exe_name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("ApplicationFrameHost.exe"))
                .unwrap_or(false)
            {
                let mut st = ChildSearch {
                    host_pid: pid,
                    found: 0,
                };
                let _ = EnumChildWindows(
                    Some(h),
                    Some(enum_child),
                    LPARAM(&mut st as *mut _ as isize),
                );
                if st.found != 0 {
                    let mut real = process_identity(st.found);
                    if real.status == IdentityStatus::Resolved {
                        real.host_exe = app.exe_name.take();
                        app = real;
                    }
                }
            }
            // Every live window has a class name; failing to read it means the window is going away.
            let Some(class) = window_class(h) else {
                return Err(DescribeError::WindowGone);
            };
            let (title, title_status) = window_title(h);
            Ok(Focus {
                app,
                window: WindowInfo {
                    hwnd,
                    class: Some(class),
                    title,
                    title_status,
                    title_truncated: false,
                },
            })
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Runtime (thread-local: hooks and window procedures are plain C callbacks without user data)

/// Optional user interface plugged into the tracker's message loop (the tray). The tracker owns
/// recording; the UI only observes state and sends commands through [`Handle`].
pub trait Ui {
    /// Called once with the hidden tracker window BEFORE any hook is installed or any event recorded.
    /// Returning `Err` makes `run` fail without recording anything: the collector never records
    /// without its visible indicator (fail closed).
    fn attach(&mut self, hwnd: isize, control: Handle) -> Result<(), String>;
    /// The collection state changed (recording / paused / locked / asleep / ...).
    fn state_changed(&mut self, state: CollectionState);
    /// A window message the tracker does not handle itself. Return `Some(result)` if handled.
    fn message(&mut self, hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> Option<isize>;
    /// Something was just made durable (flush timer / session boundary): the UI may refresh warning
    /// indicators. Event-driven: this is NOT a polling hook.
    fn activity(&mut self) {}
    /// The message loop ended (remove icons, destroy windows).
    fn detach(&mut self);
}

struct Runtime {
    engine: Engine<WinPlatform>,
    sink: Box<dyn Sink>,
    ui: Option<Box<dyn Ui>>,
    last_state: CollectionState,
    hwnd: HWND,
    flush_ms: u32,
    flush_armed: bool,
    out: Vec<Emit>,
}

thread_local! {
    static RT: RefCell<Option<Runtime>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Observation>> = const { RefCell::new(VecDeque::new()) };
    static DROPPED: Cell<u64> = const { Cell::new(0) };
    static FG: Cell<u64> = const { Cell::new(0) };
    static MSG_HWND: Cell<isize> = const { Cell::new(0) };
}

fn enqueue(o: Observation) {
    let first = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        if q.len() >= QUEUE_CAP {
            // Bounded: drop the oldest hint. Foreground state is re-synchronised from the OS on the next event.
            q.pop_front();
            DROPPED.with(|d| d.set(d.get() + 1));
        }
        let first = q.is_empty();
        q.push_back(o);
        first
    });
    if first {
        // Out-of-context hook callbacks run *inside* GetMessage without returning to our loop: nudge it.
        let h = MSG_HWND.with(|h| h.get());
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut c_void)), WM_NUDGE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let h = hwnd_u64(hwnd);
    match event {
        EVENT_SYSTEM_FOREGROUND_ => {
            FG.with(|f| f.set(h));
            enqueue(Observation::Foreground(if h == 0 { None } else { Some(h) }));
        }
        EVENT_OBJECT_NAMECHANGE_
            if id_object == 0 && id_child == 0 && h != 0 && h == FG.with(|f| f.get()) =>
        {
            enqueue(Observation::TitleHint(h));
        }
        EVENT_OBJECT_DESTROY_
            if id_object == 0 && id_child == 0 && h != 0 && h == FG.with(|f| f.get()) =>
        {
            enqueue(Observation::Destroyed(h));
        }
        _ => {}
    }
}

/// Feeds queued observations through the engine, forwards emitted events, re-arms timers.
fn pump() {
    RT.with(|rt| {
        let Ok(mut guard) = rt.try_borrow_mut() else {
            return;
        };
        let Some(r) = guard.as_mut() else { return };
        let mut durable = false;
        loop {
            let next = QUEUE.with(|q| q.borrow_mut().pop_front());
            let Some(o) = next else { break };
            durable |= needs_durable_flush(&o);
            r.engine.handle(o, &mut r.out);
        }
        finish(r);
        if durable {
            r.sink.flush();
            if let Some(ui) = r.ui.as_mut() {
                ui.activity();
            }
        }
    });
}

fn finish(r: &mut Runtime) {
    for e in r.out.drain(..) {
        r.sink.emit(e);
    }
    let st = r.engine.state();
    if st != r.last_state {
        // Only mark the state as delivered when the UI really received it: while a modal menu/dialog has
        // the UI checked out (see `call_ui`) it is re-delivered as soon as the UI is returned.
        if let Some(ui) = r.ui.as_mut() {
            ui.state_changed(st);
            r.last_state = st;
        }
    }
    unsafe {
        match r.engine.next_deadline() {
            Some(d) => {
                let now = r.engine.platform().mono_us();
                let ms = (d.saturating_sub(now) / 1000).clamp(1, 60_000) as u32;
                SetTimer(Some(r.hwnd), TIMER_TITLE, ms, None);
            }
            None => {
                let _ = KillTimer(Some(r.hwnd), TIMER_TITLE);
            }
        }
        if r.sink.has_pending() && !r.flush_armed {
            SetTimer(Some(r.hwnd), TIMER_FLUSH, r.flush_ms, None);
            r.flush_armed = true;
        }
    }
}

fn with_runtime(f: impl FnOnce(&mut Runtime)) {
    RT.with(|rt| {
        if let Ok(mut g) = rt.try_borrow_mut() {
            if let Some(r) = g.as_mut() {
                f(r);
                finish(r);
            }
        }
    });
}

/// Runs `f` on the UI with the runtime NOT borrowed (the UI may open modal menus/dialogs whose nested
/// message loops must still be able to process tracker events).
fn call_ui<R>(f: impl FnOnce(&mut dyn Ui) -> R) -> Option<R> {
    let mut ui = RT.with(|rt| rt.try_borrow_mut().ok().and_then(|mut g| g.as_mut().and_then(|r| r.ui.take())))?;
    let res = f(ui.as_mut());
    RT.with(|rt| {
        if let Ok(mut g) = rt.try_borrow_mut() {
            if let Some(r) = g.as_mut() {
                r.ui = Some(ui);
            }
        }
    });
    with_runtime(|_| {}); // deliver any state change that happened while the UI was busy
    Some(res)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NUDGE => {
            pump();
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match wp.0 as u32 {
                7 => enqueue(Observation::Lock),
                8 => enqueue(Observation::Unlock),
                6 => enqueue(Observation::Logoff),
                2 | 4 => enqueue(Observation::Disconnect),
                1 | 3 => enqueue(Observation::Reconnect),
                _ => {}
            }
            pump();
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            match wp.0 as u32 {
                4 => enqueue(Observation::Suspend),     // PBT_APMSUSPEND
                7 | 18 => enqueue(Observation::Resume), // PBT_APMRESUMESUSPEND / AUTOMATIC
                _ => {}
            }
            pump();
            LRESULT(1) // TRUE: permit
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wp.0 != 0 {
                // System is going down: close the session and make everything durable *now*.
                enqueue(Observation::Stop);
                pump();
                with_runtime(|r| r.sink.prepare_exit());
            }
            LRESULT(0)
        }
        WM_CTL_PAUSE => {
            enqueue(Observation::Pause);
            pump();
            LRESULT(0)
        }
        WM_CTL_RESUME => {
            enqueue(Observation::ResumeCollection);
            pump();
            LRESULT(0)
        }
        WM_CTL_STOP => {
            enqueue(Observation::Stop);
            pump();
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_TIMER => {
            match wp.0 {
                TIMER_TITLE => {
                    let _ = KillTimer(Some(hwnd), TIMER_TITLE);
                    with_runtime(|r| {
                        let mut out = std::mem::take(&mut r.out);
                        r.engine.tick(&mut out);
                        r.out = out;
                    });
                }
                TIMER_FLUSH => {
                    let _ = KillTimer(Some(hwnd), TIMER_FLUSH);
                    with_runtime(|r| {
                        r.flush_armed = false;
                        r.sink.flush();
                        if let Some(ui) = r.ui.as_mut() {
                            ui.activity();
                        }
                    });
                }
                _ => {
                    if let Some(Some(res)) = call_ui(|u| u.message(hwnd.0 as isize, msg, wp.0, lp.0)) {
                        return LRESULT(res);
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => match call_ui(|u| u.message(hwnd.0 as isize, msg, wp.0, lp.0)) {
            Some(Some(res)) => LRESULT(res),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Public API

pub struct TrackerConfig {
    pub engine: Config,
    /// One-shot flush delay after the first buffered event.
    pub flush_ms: u32,
    /// Ignore windows of this very process (normal); tests that create their own windows set false.
    pub skip_own_process: bool,
    /// Begin in the paused state (nothing recorded until resumed).
    pub start_paused: bool,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        TrackerConfig {
            engine: Config::default(),
            flush_ms: 2000,
            skip_own_process: true,
            start_paused: false,
        }
    }
}

/// Thread-safe remote control for the running tracker (usable from a tray UI thread later).
#[derive(Clone, Copy)]
pub struct Handle(isize);

unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    fn post(&self, msg: u32) {
        unsafe {
            let _ = PostMessageW(Some(HWND(self.0 as *mut c_void)), msg, WPARAM(0), LPARAM(0));
        }
    }
    pub fn pause(&self) {
        self.post(WM_CTL_PAUSE)
    }
    pub fn resume(&self) {
        self.post(WM_CTL_RESUME)
    }
    pub fn stop(&self) {
        self.post(WM_CTL_STOP)
    }
}

/// The control handle of a collector/tray that is already running on this desktop (any process).
pub fn running_instance() -> Option<Handle> {
    let h = unsafe { FindWindowW(w!("HermesTrackerWindow"), None) }.ok()?;
    if h.0.is_null() {
        None
    } else {
        Some(Handle(h.0 as isize))
    }
}

static CTRL_TARGET: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn ctrl_handler(ctrl: u32) -> BOOL {
    if ctrl == CTRL_C_EVENT || ctrl == CTRL_BREAK_EVENT || ctrl == CTRL_CLOSE_EVENT {
        let h = CTRL_TARGET.load(Ordering::SeqCst);
        if h != 0 {
            Handle(h).stop();
            return BOOL(1);
        }
    }
    BOOL(0)
}

/// Console Ctrl+C / Ctrl+Break / window close => graceful `stop()`.
pub fn install_console_ctrl_handler(h: Handle) {
    CTRL_TARGET.store(h.0, Ordering::SeqCst);
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
    }
}

/// Runs the tracker on the current thread until `Handle::stop` (or WM_QUIT). Blocks in
/// `GetMessageW`, i.e. uses no CPU while nothing happens.
pub fn run(
    cfg: TrackerConfig,
    sink: Box<dyn Sink>,
    mut ui: Option<Box<dyn Ui>>,
    on_ready: impl FnOnce(Handle),
) -> Result<(), String> {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.to_string())?.into();
        let class = w!("HermesTrackerWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!("RegisterClassW failed: {:?}", GetLastError()));
        }
        // Hidden top-level window (never shown): needed for WTS and power broadcasts.
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Hermes"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )
        .map_err(|e| format!("CreateWindowExW: {e}"))?;
        MSG_HWND.with(|h| h.set(hwnd.0 as isize));
        // The UI (visible recording indicator) must be in place BEFORE anything is recorded.
        if let Some(u) = ui.as_mut() {
            if let Err(e) = u.attach(hwnd.0 as isize, Handle(hwnd.0 as isize)) {
                let _ = DestroyWindow(hwnd);
                MSG_HWND.with(|h| h.set(0));
                return Err(format!("recording indicator unavailable, not recording: {e}"));
            }
        }
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

        let flags = WINEVENT_OUTOFCONTEXT_
            | if cfg.skip_own_process {
                WINEVENT_SKIPOWNPROCESS_
            } else {
                0
            };
        let hooks = [
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND_,
                EVENT_SYSTEM_FOREGROUND_,
                None,
                Some(win_event_proc),
                0,
                0,
                flags,
            ),
            SetWinEventHook(
                EVENT_OBJECT_NAMECHANGE_,
                EVENT_OBJECT_NAMECHANGE_,
                None,
                Some(win_event_proc),
                0,
                0,
                flags,
            ),
            SetWinEventHook(
                EVENT_OBJECT_DESTROY_,
                EVENT_OBJECT_DESTROY_,
                None,
                Some(win_event_proc),
                0,
                0,
                flags,
            ),
        ];
        if hooks.iter().any(|h| h.0.is_null()) {
            for h in hooks.iter().filter(|h| !h.0.is_null()) {
                let _ = UnhookWinEvent(*h);
            }
            if let Some(u) = ui.as_mut() {
                u.detach();
            }
            let _ = DestroyWindow(hwnd);
            return Err("SetWinEventHook failed".into());
        }

        let mut engine = Engine::new(WinPlatform::new(), cfg.engine);
        let mut out = Vec::new();
        FG.with(|f| f.set(engine.platform().foreground().unwrap_or(0)));
        if cfg.start_paused {
            engine.start_paused();
        } else {
            engine.start(&mut out);
        }
        RT.with(|rt| {
            *rt.borrow_mut() = Some(Runtime {
                engine,
                sink,
                ui,
                last_state: CollectionState::Stopped,
                hwnd,
                flush_ms: cfg.flush_ms,
                flush_armed: false,
                out,
            })
        });
        with_runtime(|_| {}); // forward the start events, report the initial state to the UI, arm timers

        on_ready(Handle(hwnd.0 as isize));

        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 <= 0 {
                break; // WM_QUIT (0) or error (-1)
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        for h in hooks {
            let _ = UnhookWinEvent(h);
        }
        let _ = WTSUnRegisterSessionNotification(hwnd);
        // Final durability point (the engine already emitted SESSION_END on stop).
        with_runtime(|r| r.sink.flush());
        let ui = RT.with(|rt| rt.borrow_mut().take().and_then(|r| r.ui));
        if let Some(mut u) = ui {
            u.detach();
        }
        let _ = DestroyWindow(hwnd);
        MSG_HWND.with(|h| h.set(0));
    }
    Ok(())
}

/// Observations dropped because the bounded queue overflowed (diagnostics).
pub fn dropped_observations() -> u64 {
    DROPPED.with(|d| d.get())
}
