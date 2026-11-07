//! Where finished events go. Platform independent so the collector's storage glue is testable anywhere.

use crate::engine::Emit;

/// Receives finished events from the tracker. Called only on the tracker thread.
pub trait Sink {
    fn emit(&mut self, e: Emit);
    /// Make buffered events durable. Called from a one-shot timer and at session boundaries, never in a polling loop.
    fn flush(&mut self);
    /// True if there is buffered data that a flush timer should be armed for.
    fn has_pending(&self) -> bool;
    /// Last chance before the process may be killed (logoff/shutdown): flush AND mark the store cleanly closed.
    fn prepare_exit(&mut self) {
        self.flush();
    }
}
