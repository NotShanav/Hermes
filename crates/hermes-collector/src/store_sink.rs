use crate::status::Status;
use hermes_db::{EventStore, PushOutcome};
use hermes_tracker::{Emit, Sink};
use std::cell::RefCell;
use std::rc::Rc;

/// Connects tracker events to the encrypted store and keeps the user-visible status current.
pub struct StoreSink {
    store: Rc<RefCell<EventStore>>,
    status: Rc<RefCell<Status>>,
    quiet: bool,
}

impl StoreSink {
    pub fn new(store: Rc<RefCell<EventStore>>, status: Rc<RefCell<Status>>, quiet: bool) -> Self {
        StoreSink { store, status, quiet }
    }

    fn record_error(&self, msg: String) {
        let mut s = self.status.borrow_mut();
        s.store_errors += 1;
        s.last_error = Some(msg);
    }
}

impl Sink for StoreSink {
    fn emit(&mut self, e: Emit) {
        if !self.quiet {
            eprintln!("[{}] kind=0x{:04x} session={:016x}", e.ts_us, e.kind, e.session_id);
        }
        let res = self.store.borrow_mut().push(e.session_id, e.ts_us, e.kind, &e.body);
        match res {
            Ok(PushOutcome::Dropped) => {
                self.status.borrow_mut().events_dropped += 1;
                self.record_error("event buffer full: storage is not accepting writes".into());
            }
            Ok(_) => self.status.borrow_mut().events_recorded += 1,
            Err(err) => {
                self.status.borrow_mut().events_dropped += 1;
                self.record_error(err.to_string());
            }
        }
    }

    fn flush(&mut self) {
        let res = self.store.borrow_mut().flush();
        match res {
            Ok(()) => {
                let t = self.store.borrow().last_flush_us();
                let mut s = self.status.borrow_mut();
                if t != 0 {
                    s.last_flush_us = t;
                }
                s.last_error = None; // storage is working again; counters keep the history
            }
            Err(err) => self.record_error(format!("flush failed (will retry): {err}")),
        }
    }

    fn has_pending(&self) -> bool {
        self.store.borrow().buffered_events() > 0
    }

    fn prepare_exit(&mut self) {
        self.flush();
        if let Err(e) = self.store.borrow().database().mark_clean_shutdown() {
            self.record_error(format!("could not mark clean shutdown: {e}"));
        }
    }
}
