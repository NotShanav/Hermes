//! Hermes Collector core: opens the archive for writing and turns tracker events into encrypted, signed,
//! batched database rows. UI-free and platform-independent (the console binary and the tray both use it).
//!
//! The collector holds only the PUBLIC key and the signing key: it cannot decrypt what it writes.

#[cfg(windows)]
pub mod console;
mod status;
mod store_sink;

pub use status::{format_status, warnings, Status};
pub use store_sink::StoreSink;

use hermes_crypto::provision::load_signer;
use hermes_crypto::SecretProtector;
use hermes_db::{Database, EventStore, OpenOptions, StoreConfig};
use hermes_tracker::Sink;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub struct Collector {
    store: Rc<RefCell<EventStore>>,
    status: Rc<RefCell<Status>>,
    data_dir: PathBuf,
}

/// Summary after a graceful close.
#[derive(Debug, Clone)]
pub struct FinishInfo {
    pub events_recorded: u64,
    pub events_dropped: u64,
}

impl Collector {
    /// Opens `<data_dir>/hermes.db`, loads the signing key through `protector` (DPAPI on Windows) and
    /// prepares the encrypted store. Fails (recording nothing) if no signed key set is registered.
    pub fn open(data_dir: &Path, protector: &dyn SecretProtector, exclusions: usize) -> Result<Collector, String> {
        let db_path = data_dir.join("hermes.db");
        let (db, report) = Database::open(&db_path, OpenOptions::default()).map_err(|e| e.to_string())?;
        let key = db.active_key().map_err(|e| e.to_string())?.ok_or("no key set registered: run `hermes-keytool init` first")?;
        let signer = load_signer(&data_dir.join("keys"), &key.info.key_id, protector)
            .map_err(|e| format!("cannot load the signing key for key set {} ({e}); run `hermes-keytool rotate`", key.info.key_id))?;
        let status = Status {
            data_dir: data_dir.to_path_buf(),
            key_id: key.info.key_id.to_hex(),
            unclean_previous_shutdown: report.unclean_previous_shutdown,
            integrity_check_ran: report.ran_integrity_check,
            repaired: report.recovery.as_ref().map(|r| format!("original kept at {} (salvaged {} events, complete={})", r.quarantined_to.display(), r.salvaged_events, r.salvage_complete)),
            exclusions,
            ..Status::default()
        };
        let store = EventStore::from_database(db, report, signer, StoreConfig::default()).map_err(|e| e.to_string())?;
        Ok(Collector { store: Rc::new(RefCell::new(store)), status: Rc::new(RefCell::new(status)), data_dir: data_dir.to_path_buf() })
    }

    pub fn sink(&self, quiet: bool) -> Box<dyn Sink> {
        Box::new(StoreSink::new(self.store.clone(), self.status.clone(), quiet))
    }

    /// Shared, live status (the tray reads it when its menu opens).
    pub fn status(&self) -> Rc<RefCell<Status>> {
        self.status.clone()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Graceful close: flush, mark the database cleanly closed.
    pub fn finish(self) -> Result<FinishInfo, String> {
        let st = self.status.borrow().clone();
        let store = Rc::try_unwrap(self.store).map_err(|_| "internal: store still in use")?.into_inner();
        store.close().map_err(|e| e.to_string())?;
        Ok(FinishInfo { events_recorded: st.events_recorded, events_dropped: st.events_dropped })
    }
}
