//! Hermes viewer core. Unlocking needs BOTH key shares (Share A through the OS protector, Share B from the
//! USB file). Decrypted data lives only in this process's memory; nothing is written to disk unless the user
//! explicitly exports (and exports are plaintext).

mod export;
mod model;
mod time;

pub use export::{export_csv, export_json};
pub use model::{AppTime, Archive, Filter, ViewEvent, ViewKind};
pub use time::{date_range, fmt_offset, fmt_time, parse_day};
