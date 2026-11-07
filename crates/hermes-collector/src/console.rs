//! Foreground console collector (Windows). Shared by `hermes-collector run` and `hermes start`.

use crate::Collector;
use hermes_platform::single::acquire;
use hermes_tracker::win::{self, TrackerConfig};
use hermes_tracker::{Config, DEFAULT_EXCLUSIONS};
use std::path::PathBuf;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

pub struct ConsoleOptions {
    pub data_dir: PathBuf,
    pub exclude: Vec<String>,
    pub default_exclusions: bool,
    pub quiet: bool,
    pub duration_secs: Option<u64>,
}

fn stats() -> String {
    unsafe {
        let p = GetCurrentProcess();
        let mut m = PROCESS_MEMORY_COUNTERS_EX::default();
        m.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = GetProcessMemoryInfo(p, &mut m as *mut _ as *mut _, m.cb).is_ok();
        let (mut c, mut x, mut k, mut u) = Default::default();
        let _ = GetProcessTimes(p, &mut c, &mut x, &mut k, &mut u);
        let cpu = |f: windows::Win32::Foundation::FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 1e7;
        if ok {
            format!(
                "working_set={:.1} MiB  peak_working_set={:.1} MiB  private={:.1} MiB  cpu_user={:.3}s cpu_kernel={:.3}s",
                m.WorkingSetSize as f64 / 1048576.0,
                m.PeakWorkingSetSize as f64 / 1048576.0,
                m.PrivateUsage as f64 / 1048576.0,
                cpu(u),
                cpu(k)
            )
        } else {
            "memory counters unavailable".into()
        }
    }
}

/// Runs the collector in the foreground until Ctrl+C / `hermes stop` / the optional duration elapses.
pub fn run(o: ConsoleOptions) -> Result<(), String> {
    let _guard = match acquire(&o.data_dir)? {
        Some(g) => g,
        None => return Err(format!("another Hermes collector/tray is already running for {}", o.data_dir.display())),
    };
    let prot = hermes_platform::default_protector().map_err(|e| e.to_string())?;
    let mut exclusions: Vec<String> = if o.default_exclusions { DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect() } else { vec![] };
    exclusions.extend(o.exclude.iter().cloned());
    let collector = Collector::open(&o.data_dir, prot.as_ref(), exclusions.len())?;
    {
        let st = collector.status();
        let st = st.borrow();
        for w in crate::warnings(&st) {
            eprintln!("NOTICE: {w}");
        }
        eprintln!("Hermes {} is RECORDING foreground application names and window titles to {}", env!("CARGO_PKG_VERSION"), o.data_dir.join("hermes.db").display());
        eprintln!("Key set {}; {} application exclusion(s). Everything is encrypted before it is written;", st.key_id, exclusions.len());
        eprintln!("this program cannot read it back. No keystrokes, text or screen content are collected.");
        eprintln!("Press Ctrl+C (or run `hermes stop` from another window) to stop.");
    }
    let cfg = TrackerConfig { engine: Config { exclusions, ..Config::default() }, ..TrackerConfig::default() };
    let duration = o.duration_secs;
    win::run(cfg, collector.sink(o.quiet), None, move |h| {
        win::install_console_ctrl_handler(h);
        if let Some(s) = duration {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(s));
                h.stop();
            });
        }
    })?;
    let dropped = win::dropped_observations();
    let info = collector.finish()?;
    eprintln!("Stopped cleanly. Recorded {} events ({} dropped, {dropped} dropped observations).", info.events_recorded, info.events_dropped);
    eprintln!("{}", stats());
    Ok(())
}
