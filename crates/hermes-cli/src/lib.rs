//! Implementation of the `hermes` command. Everything except process control (start/stop/tray) is portable
//! and unit-tested; the protector (DPAPI on Windows) is injected.

pub mod args;
pub mod init;
pub mod repl;
pub mod status;
pub mod usb;
