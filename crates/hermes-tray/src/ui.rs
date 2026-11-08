//! Win32 shell of the tray: notification-area icon, popup menu, on-screen recording dot, status dialog.
//! Event-driven: it reacts to messages and to state changes pushed by the tracker; it owns no timers
//! except the user-requested one-shot "pause for N minutes" timer. No tracking or storage logic here.
//!
//! FAIL CLOSED: `attach` fails (so the collector records nothing) if the icon cannot be shown, and a
//! lost icon that cannot be restored pauses recording.
#![allow(unsafe_code)]

use crate::icon::{self, IconKind};
use crate::live;
use crate::menu::{self, ids, Item, MenuInput};
use crate::settings::Settings;
use hermes_collector::{format_status, warnings, Status};
use hermes_tracker::win::{Handle, Ui};
use hermes_tracker::CollectionState;
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateEllipticRgn, CreateSolidBrush, DeleteObject, InvalidateRect, SetWindowRgn, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 40;
const TIMER_AUTO_RESUME: usize = 100;
const DOT_SIZE: i32 = 12;
const NIIF_INFO_: u32 = 1;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn hwnd(h: isize) -> HWND {
    HWND(h as *mut c_void)
}
fn copy_utf16(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

pub struct TrayUi {
    hwnd: isize,
    ctl: Option<Handle>,
    state: CollectionState,
    settings: Settings,
    settings_path: PathBuf,
    data_dir: PathBuf,
    status: Rc<RefCell<Status>>,
    icons: Vec<(IconKind, HICON)>,
    shown: Option<(IconKind, String)>,
    dot: Option<HWND>,
    taskbar_created: u32,
    pause_note: Option<String>,
    paused_by_indicator: bool,
    announced: bool,
    exe_path: PathBuf,
}

impl TrayUi {
    pub fn new(settings: Settings, settings_path: PathBuf, data_dir: PathBuf, status: Rc<RefCell<Status>>) -> Self {
        TrayUi {
            hwnd: 0,
            ctl: None,
            state: CollectionState::Stopped,
            settings,
            settings_path,
            data_dir,
            status,
            icons: vec![],
            shown: None,
            dot: None,
            taskbar_created: 0,
            pause_note: None,
            paused_by_indicator: false,
            announced: false,
            exe_path: std::env::current_exe().unwrap_or_default(),
        }
    }

    // ------------------------------------------------------------------ icon
    fn make_icon(kind: IconKind, size: i32) -> Result<HICON, String> {
        let px = icon::pixels(kind, size as u32);
        unsafe {
            let color = CreateBitmap(size, size, 1, 32, Some(px.as_ptr() as *const c_void));
            let mask = CreateBitmap(size, size, 1, 1, None);
            let ii = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
            let h = CreateIconIndirect(&ii).map_err(|e| format!("CreateIconIndirect: {e}"));
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(mask.into());
            h
        }
    }

    fn icon_for(&self, kind: IconKind) -> HICON {
        self.icons.iter().find(|(k, _)| *k == kind).map(|(_, h)| *h).unwrap_or_default()
    }

    fn nid(&self, flags: windows::Win32::UI::Shell::NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd(self.hwnd), uID: 1, uFlags: flags, ..Default::default() }
    }

    fn summary(&self) -> String {
        let s = self.status.borrow();
        menu::summary(s.events_recorded, s.events_dropped, s.last_flush_us)
    }

    fn desired(&self) -> (IconKind, String) {
        let warn = !warnings(&self.status.borrow()).is_empty();
        (icon::kind_for(self.state, warn), menu::tooltip(self.state, &self.summary()))
    }

    /// Adds the notification-area icon. Returns false if the shell refuses.
    fn add_icon(&mut self) -> bool {
        let (kind, tip) = self.desired();
        let mut d = self.nid(NIF_ICON | NIF_MESSAGE | NIF_TIP);
        d.hIcon = self.icon_for(kind);
        d.uCallbackMessage = WM_TRAY;
        copy_utf16(&mut d.szTip, &tip);
        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
                return false;
            }
            d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
        }
        self.shown = Some((kind, tip));
        true
    }

    /// Brings the icon/tooltip in line with the current state. If the icon has vanished and cannot be
    /// restored, recording is paused: Hermes never records without a visible indicator.
    fn refresh_icon(&mut self) {
        let want = self.desired();
        if self.shown.as_ref() == Some(&want) {
            return;
        }
        let mut d = self.nid(NIF_ICON | NIF_TIP);
        d.hIcon = self.icon_for(want.0);
        copy_utf16(&mut d.szTip, &want.1);
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &d).as_bool() };
        if ok {
            self.shown = Some(want);
            self.set_dot_color(self.shown.as_ref().map(|s| s.0).unwrap_or(IconKind::Recording));
        } else {
            self.indicator_lost();
        }
    }

    fn indicator_lost(&mut self) {
        self.shown = None;
        if self.add_icon() {
            return;
        }
        if self.state == CollectionState::Recording {
            self.paused_by_indicator = true;
            if let Some(c) = &self.ctl {
                c.pause();
            }
            self.message_box("Hermes can no longer show its notification-area icon, so recording has been PAUSED.\n\nIt resumes automatically when the icon can be shown again.", "Hermes", MB_ICONWARNING);
        }
    }

    fn balloon(&self, title: &str, text: &str) {
        let mut d = self.nid(NIF_INFO);
        copy_utf16(&mut d.szInfoTitle, title);
        copy_utf16(&mut d.szInfo, text);
        d.dwInfoFlags = windows::Win32::UI::Shell::NOTIFY_ICON_INFOTIP_FLAGS(NIIF_INFO_);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
        }
    }

    fn message_box(&self, text: &str, title: &str, icon: MESSAGEBOX_STYLE) {
        let (t, c) = (wide(text), wide(title));
        unsafe {
            MessageBoxW(Some(hwnd(self.hwnd)), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_OK | icon);
        }
    }

    // ------------------------------------------------------------------ recording dot
    fn create_dot(&mut self) {
        unsafe {
            let hinst = GetModuleHandleW(None).ok().map(|m| m.into());
            let class = w!("HermesRecordingDot");
            let wc = WNDCLASSW { lpfnWndProc: Some(dot_proc), hInstance: hinst.unwrap_or_default(), lpszClassName: class, hbrBackground: CreateSolidBrush(rgb(icon::RED)), ..Default::default() };
            RegisterClassW(&wc); // already-registered is fine
            if let Ok(h) = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class, w!("Hermes is recording"), WS_POPUP, 0, 0, DOT_SIZE, DOT_SIZE, None, None, hinst, None) {
                let rgn = CreateEllipticRgn(0, 0, DOT_SIZE + 1, DOT_SIZE + 1);
                SetWindowRgn(h, Some(rgn), true);
                self.dot = Some(h);
                self.position_dot();
            }
        }
    }

    fn position_dot(&self) {
        if let Some(h) = self.dot {
            unsafe {
                let x = GetSystemMetrics(SM_CXSCREEN) - DOT_SIZE - 8;
                let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, 4, DOT_SIZE, DOT_SIZE, SWP_NOACTIVATE);
            }
        }
    }

    fn set_dot_color(&self, kind: IconKind) {
        if let Some(h) = self.dot {
            let c = if kind == IconKind::Warning { icon::AMBER } else { icon::RED };
            unsafe {
                SetClassLongPtrW(h, GCLP_HBRBACKGROUND, CreateSolidBrush(rgb(c)).0 as isize);
                let _ = InvalidateRect(Some(h), None, true);
            }
        }
    }

    fn refresh_dot(&mut self) {
        let want = self.settings.show_indicator_dot && self.state == CollectionState::Recording;
        if want && self.dot.is_none() {
            self.create_dot();
        }
        if let Some(h) = self.dot {
            unsafe {
                let _ = ShowWindow(h, if want { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
            if want {
                self.position_dot();
                self.set_dot_color(self.shown.as_ref().map(|s| s.0).unwrap_or(IconKind::Recording));
            }
        }
    }

    // ------------------------------------------------------------------ menu & commands
    /// Publishes the collection state for the desktop app (counters only; best effort).
    fn publish_live(&self, state: CollectionState) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0);
        let snap = live::snapshot(state, &self.status.borrow(), now);
        let _ = live::write(&live::live_path(&self.data_dir), &snap);
    }

    fn autostart_command(&self) -> String {
        menu::autostart_command(&self.exe_path.to_string_lossy(), &self.data_dir.to_string_lossy())
    }

    fn viewer_path(&self) -> PathBuf {
        self.exe_path.with_file_name("hermes-viewer.exe")
    }

    fn show_menu(&mut self) {
        let input = MenuInput {
            state: self.state,
            summary: self.summary(),
            warnings: warnings(&self.status.borrow()).len(),
            autostart: crate::autostart::is_enabled(),
            start_on_launch: self.settings.start_collecting_on_launch,
            dot: self.settings.show_indicator_dot,
            viewer_available: self.viewer_path().exists(),
            pause_note: self.pause_note.clone(),
        };
        let items = menu::build(&input);
        unsafe {
            let Ok(hm) = CreatePopupMenu() else { return };
            for Item { id, label, enabled, checked, separator } in &items {
                if *separator {
                    let _ = AppendMenuW(hm, MF_SEPARATOR, 0, PCWSTR::null());
                } else {
                    let mut flags = MF_STRING;
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    if *checked {
                        flags |= MF_CHECKED;
                    }
                    let l = wide(label);
                    let _ = AppendMenuW(hm, flags, *id as usize, PCWSTR(l.as_ptr()));
                }
            }
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(hwnd(self.hwnd)); // required so the menu closes when clicking elsewhere
            let cmd = TrackPopupMenu(hm, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, Some(0), hwnd(self.hwnd), None);
            let _ = PostMessageW(Some(hwnd(self.hwnd)), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(hm);
            if cmd.0 > 0 {
                self.command(cmd.0 as u32);
            }
        }
    }

    fn save_settings(&self) {
        if let Err(e) = self.settings.save(&self.settings_path) {
            self.message_box(&format!("Could not save settings: {e}"), "Hermes", MB_ICONWARNING);
        }
    }

    fn shell_open(&self, target: &std::path::Path) {
        let t = wide(&target.to_string_lossy());
        unsafe {
            ShellExecuteW(Some(hwnd(self.hwnd)), w!("open"), PCWSTR(t.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }

    fn cancel_auto_resume(&mut self) {
        unsafe {
            let _ = KillTimer(Some(hwnd(self.hwnd)), TIMER_AUTO_RESUME);
        }
        self.pause_note = None;
    }

    fn pause_for(&mut self, minutes: u32) {
        if let Some(c) = &self.ctl {
            c.pause();
        }
        unsafe {
            SetTimer(Some(hwnd(self.hwnd)), TIMER_AUTO_RESUME, minutes * 60_000, None);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) + minutes as u64 * 60;
        self.pause_note = Some(format!("auto-resumes at {:02}:{:02} UTC", now / 3600 % 24, now / 60 % 60));
    }

    /// Executes a menu command. Also reachable by posting WM_COMMAND (used by the automated tests).
    pub fn command(&mut self, id: u32) {
        match id {
            ids::PAUSE_RESUME => match self.state {
                CollectionState::Paused => {
                    self.cancel_auto_resume();
                    if let Some(c) = &self.ctl {
                        c.resume();
                    }
                }
                CollectionState::Recording => {
                    if let Some(c) = &self.ctl {
                        c.pause();
                    }
                }
                _ => {}
            },
            ids::PAUSE_15 => self.pause_for(15),
            ids::PAUSE_60 => self.pause_for(60),
            ids::STATUS => {
                let st = menu::state_label(self.state);
                let text = format_status(&self.status.borrow(), st, crate::autostart::is_enabled());
                self.message_box(&text, "Hermes status and security", MB_ICONINFORMATION);
            }
            ids::OPEN_VIEWER => {
                let v = self.viewer_path();
                if v.exists() {
                    self.shell_open(&v);
                }
            }
            ids::AUTOSTART => {
                let on = !crate::autostart::is_enabled();
                if let Err(e) = crate::autostart::set(on, &self.autostart_command()) {
                    self.message_box(&format!("Could not change the startup setting: {e}"), "Hermes", MB_ICONWARNING);
                }
            }
            ids::START_ON_LAUNCH => {
                self.settings.start_collecting_on_launch = !self.settings.start_collecting_on_launch;
                self.save_settings();
            }
            ids::DOT => {
                self.settings.show_indicator_dot = !self.settings.show_indicator_dot;
                self.save_settings();
                self.refresh_dot();
            }
            ids::OPEN_FOLDER => self.shell_open(&self.data_dir.clone()),
            ids::OPEN_SETTINGS => {
                if !self.settings_path.exists() {
                    self.save_settings();
                }
                self.shell_open(&self.settings_path.clone());
            }
            ids::EXIT => {
                if let Some(c) = &self.ctl {
                    c.stop();
                }
            }
            _ => {}
        }
    }
}

fn rgb(c: (u8, u8, u8)) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(c.0 as u32 | (c.1 as u32) << 8 | (c.2 as u32) << 16)
}

unsafe extern "system" fn dot_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_RBUTTONUP | WM_LBUTTONUP => {
            // Clicking the dot opens the same menu as the tray icon.
            let main = MAIN_HWND.load(Ordering::SeqCst);
            if main != 0 {
                let _ = PostMessageW(Some(hwnd(main)), WM_TRAY, WPARAM(0), LPARAM(WM_CONTEXTMENU as isize));
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

impl Ui for TrayUi {
    fn attach(&mut self, hwnd_: isize, control: Handle) -> Result<(), String> {
        self.hwnd = hwnd_;
        self.ctl = Some(control);
        MAIN_HWND.store(hwnd_, Ordering::SeqCst);
        unsafe {
            self.taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
            let size = GetSystemMetrics(SM_CXSMICON).max(16);
            for k in [IconKind::Recording, IconKind::Paused, IconKind::Inactive, IconKind::Warning] {
                self.icons.push((k, Self::make_icon(k, size)?));
            }
        }
        self.state = CollectionState::Stopped; // until the tracker reports the real state
        if !self.add_icon() {
            self.detach();
            return Err("the notification-area (system tray) icon could not be created".into());
        }
        Ok(())
    }

    fn state_changed(&mut self, state: CollectionState) {
        let prev = self.state;
        self.state = state;
        if state != CollectionState::Paused {
            self.cancel_auto_resume();
        }
        if state == CollectionState::Recording {
            self.paused_by_indicator = false;
        }
        self.refresh_icon();
        self.refresh_dot();
        self.publish_live(state);
        if state == CollectionState::Recording && !self.announced {
            self.announced = true;
            self.balloon("Hermes is recording", "Right-click the icon to pause, see status, or exit.");
        } else if state == CollectionState::Recording && prev == CollectionState::Paused {
            self.balloon("Hermes resumed recording", "Recording is active again.");
        }
    }

    fn activity(&mut self) {
        self.refresh_icon(); // warning indicator follows storage health
        self.publish_live(self.state);
    }

    fn message(&mut self, hwnd_: isize, msg: u32, wp: usize, lp: isize) -> Option<isize> {
        match msg {
            WM_TRAY => {
                let event = (lp as u32) & 0xFFFF;
                if event == WM_CONTEXTMENU || event == WM_RBUTTONUP || event == WM_LBUTTONUP || event == 0x400 /* NIN_SELECT */ || event == 0x401 /* NIN_KEYSELECT */ {
                    self.show_menu();
                }
                Some(0)
            }
            WM_COMMAND => {
                self.command((wp & 0xFFFF) as u32);
                Some(0)
            }
            WM_TIMER if wp == TIMER_AUTO_RESUME => {
                self.cancel_auto_resume();
                if self.state == CollectionState::Paused {
                    if let Some(c) = &self.ctl {
                        c.resume();
                    }
                }
                Some(0)
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                self.position_dot();
                None
            }
            m if m != 0 && m == self.taskbar_created => {
                // Explorer restarted: the icon is gone and must be re-added.
                self.shown = None;
                if self.add_icon() {
                    if self.paused_by_indicator {
                        self.paused_by_indicator = false;
                        if let Some(c) = &self.ctl {
                            c.resume();
                        }
                    }
                } else {
                    self.indicator_lost();
                }
                let _ = hwnd_;
                Some(0)
            }
            _ => None,
        }
    }

    fn detach(&mut self) {
        self.publish_live(CollectionState::Stopped);
        unsafe {
            let d = self.nid(NIF_ICON);
            let _ = Shell_NotifyIconW(NIM_DELETE, &d);
            if let Some(h) = self.dot.take() {
                let _ = DestroyWindow(h);
            }
            for (_, i) in self.icons.drain(..) {
                let _ = DestroyIcon(i);
            }
        }
        MAIN_HWND.store(0, Ordering::SeqCst);
    }
}

#[allow(dead_code)]
fn _unused(_: HBRUSH) {}
