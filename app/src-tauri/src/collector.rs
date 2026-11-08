//! Controlling the recorder (`hermes-tray.exe`) from the app. The app never records anything itself:
//! it starts the tray collector as a separate, visible process and sends it pause / resume / stop.

use std::path::{Path, PathBuf};

#[cfg(windows)]
pub fn running() -> bool {
    hermes_tracker::win::running_instance().is_some()
}
#[cfg(not(windows))]
pub fn running() -> bool {
    false
}

pub fn tray_exe() -> PathBuf {
    std::env::current_exe().map(|p| p.with_file_name("hermes-tray.exe")).unwrap_or_else(|_| PathBuf::from("hermes-tray.exe"))
}

#[cfg(windows)]
fn spawn_tray(data_dir: &Path, paused: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let exe = tray_exe();
    if !exe.exists() {
        return Err(format!("{} was not found. Keep hermes-tray.exe in the same folder as this app.", exe.display()));
    }
    let mut c = std::process::Command::new(&exe);
    c.arg("--data-dir").arg(data_dir);
    if paused {
        c.arg("--paused");
    }
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: the recorder keeps running when this window is closed.
    c.creation_flags(0x0000_0008 | 0x0000_0200);
    c.spawn().map(|_| ()).map_err(|e| format!("could not start {}: {e}", exe.display()))
}
#[cfg(not(windows))]
fn spawn_tray(_data_dir: &Path, _paused: bool) -> Result<(), String> {
    Err("Recording needs Windows.".into())
}

#[cfg(windows)]
fn stop_and_wait() -> Result<(), String> {
    match hermes_tracker::win::running_instance() {
        None => Ok(()),
        Some(h) => {
            h.stop();
            for _ in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if hermes_tracker::win::running_instance().is_none() {
                    return Ok(());
                }
            }
            Err("The recorder was asked to stop but is still running after 10 seconds.".into())
        }
    }
}
#[cfg(not(windows))]
fn stop_and_wait() -> Result<(), String> {
    Ok(())
}

/// action: start | start_paused | pause | resume | stop | restart
pub fn control(action: &str, data_dir: &Path) -> Result<String, String> {
    match action {
        "start" | "start_paused" => {
            if running() {
                return Ok("The recorder is already running.".into());
            }
            spawn_tray(data_dir, action == "start_paused")?;
            Ok("Recorder started. Its icon is in the notification area.".into())
        }
        "restart" => {
            stop_and_wait()?;
            spawn_tray(data_dir, false)?;
            Ok("Recorder restarted with the current settings.".into())
        }
        "stop" => {
            stop_and_wait()?;
            Ok("Recorder stopped. Everything was flushed and the archive closed cleanly.".into())
        }
        "pause" | "resume" => {
            #[cfg(windows)]
            {
                match hermes_tracker::win::running_instance() {
                    None => Err("The recorder is not running.".into()),
                    Some(h) => {
                        if action == "pause" {
                            h.pause();
                            Ok("Paused. Nothing is recorded until you resume.".into())
                        } else {
                            h.resume();
                            Ok("Recording resumed.".into())
                        }
                    }
                }
            }
            #[cfg(not(windows))]
            {
                Err("Recording needs Windows.".into())
            }
        }
        other => Err(format!("unknown action {other}")),
    }
}

#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    hermes_tray::autostart::is_enabled()
}
#[cfg(not(windows))]
pub fn autostart_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_autostart(on: bool, data_dir: &Path) -> Result<(), String> {
    let cmd = hermes_tray::menu::autostart_command(&tray_exe().to_string_lossy(), &data_dir.to_string_lossy());
    hermes_tray::autostart::set(on, &cmd)
}
#[cfg(not(windows))]
pub fn set_autostart(_on: bool, _data_dir: &Path) -> Result<(), String> {
    Err("Start with Windows needs Windows.".into())
}
