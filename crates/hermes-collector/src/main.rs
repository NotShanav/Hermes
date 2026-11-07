//! Hermes Collector (milestone 1: console build).
//!
//! Records foreground-application activity into the encrypted archive. It needs only:
//!   * the PUBLIC key registered in the database (to encrypt), and
//!   * the DPAPI-protected signing key (to sign checkpoints).
//! It never reads Share A, never sees Share B and cannot decrypt anything it writes.
//!
//!   hermes-collector run [--data-dir D] [--exclude app.exe]... [--no-default-exclusions] [--quiet]
//!
//! Stop with Ctrl+C (graceful: closes the session, flushes, marks the database cleanly closed).

#[cfg(not(windows))]
fn main() {
    eprintln!("hermes-collector records Windows foreground activity and only runs on Windows.");
    eprintln!(
        "(The platform-independent engine is tested on every OS: `cargo test -p hermes-tracker`.)"
    );
    std::process::exit(2);
}

#[cfg(windows)]
mod app;

#[cfg(windows)]
fn main() {
    std::process::exit(app::main());
}
