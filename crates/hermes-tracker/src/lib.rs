//! Foreground tracking (Beta 0.1, milestone 1).
//!
//! * [`engine`]: OS-independent state machine (tested on every platform).
//! * [`win`]: event-driven Windows backend (SetWinEventHook out-of-context, WTS session
//!   notifications, power broadcasts). No polling, no injection, no keyboard/mouse hooks.
//!
//! Typed-text collection is NOT part of this crate/milestone.
#![cfg_attr(not(windows), forbid(unsafe_code))]

pub mod engine;
pub mod model;
pub mod sink;

#[cfg(windows)]
pub mod win;

pub use engine::{needs_durable_flush, CollectionState, Config, DescribeError, Emit, Engine, Observation, Platform, DEFAULT_EXCLUSIONS};
pub use sink::Sink;
pub use model::*;

#[cfg(test)]
mod tests;
