//! Hermes tray: user interface for the collector. It contains NO tracking or storage logic: it renders
//! state reported by the tracker, shows status from `hermes-collector`, and sends pause/resume/stop
//! commands through the tracker's control handle.
//!
//! Pure modules (settings, icon, menu) are portable and unit-tested everywhere; `ui` is the Win32 shell.

pub mod icon;
pub mod live;
pub mod menu;
pub mod settings;

#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod ui;

#[cfg(test)]
mod tests;
