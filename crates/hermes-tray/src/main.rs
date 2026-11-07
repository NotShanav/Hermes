//! hermes-tray: the Hermes collector with its visible system-tray control.
//!
//!   hermes-tray [--data-dir D] [--paused] [--duration-secs N]
//!
//! It is the same collector as `hermes-collector run` (shared library), plus a tray icon, an optional
//! on-screen dot and a menu. There is no hidden mode: if the tray icon cannot be shown it does not record.
//! `--duration-secs` exists for automated tests and benchmarks (graceful stop after N seconds).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("hermes-tray is the Windows system-tray collector and only runs on Windows.");
    eprintln!("(Its menu, icon and settings logic is tested on every OS: `cargo test -p hermes-tray`.)");
    std::process::exit(2);
}

#[cfg(windows)]
mod app {
    use hermes_collector::Collector;
    use hermes_platform::single::acquire;
    use hermes_tracker::win::{self, TrackerConfig};
    use hermes_tracker::Config;
    use hermes_tray::settings::Settings;
    use hermes_tray::ui::TrayUi;
    use std::path::PathBuf;
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK};

    fn show(text: &str, error: bool) {
        let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let c: Vec<u16> = "Hermes".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_OK | if error { MB_ICONERROR } else { MB_ICONINFORMATION });
        }
    }

    struct Args {
        data_dir: PathBuf,
        paused: bool,
        duration: Option<u64>,
    }

    fn parse() -> Result<Args, String> {
        let default_dir = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Hermes")).unwrap_or_else(|| PathBuf::from("Hermes"));
        let mut a = Args { data_dir: default_dir, paused: false, duration: None };
        let mut it = std::env::args().skip(1);
        while let Some(f) = it.next() {
            match f.as_str() {
                "--data-dir" => a.data_dir = it.next().ok_or("--data-dir needs a value")?.into(),
                "--paused" => a.paused = true,
                "--duration-secs" => a.duration = Some(it.next().ok_or("--duration-secs needs a value")?.parse().map_err(|_| "--duration-secs must be a number")?),
                other => return Err(format!("unknown option {other}\nusage: hermes-tray [--data-dir D] [--paused] [--duration-secs N]")),
            }
        }
        Ok(a)
    }

    pub fn main() -> i32 {
        match run() {
            Ok(code) => code,
            Err(e) => {
                show(&e, true);
                1
            }
        }
    }

    fn run() -> Result<i32, String> {
        let a = parse()?;
        let _guard = match acquire(&a.data_dir)? {
            Some(g) => g,
            None => {
                show("Hermes is already running for this data folder. Look for its icon in the notification area.", false);
                return Ok(1);
            }
        };
        let settings_path = a.data_dir.join("settings.json");
        let (settings, warning) = Settings::load(&settings_path);
        let prot = hermes_platform::default_protector().map_err(|e| e.to_string())?;
        let exclusions = settings.effective_exclusions();
        let collector = Collector::open(&a.data_dir, prot.as_ref(), exclusions.len())?;
        if let Some(w) = warning {
            show(&w, false);
        }

        let cfg = TrackerConfig { engine: Config { exclusions, ..Config::default() }, start_paused: a.paused || !settings.start_collecting_on_launch, ..TrackerConfig::default() };
        let ui = TrayUi::new(settings, settings_path, a.data_dir.clone(), collector.status());
        let duration = a.duration;
        let res = win::run(cfg, collector.sink(true), Some(Box::new(ui)), move |h| {
            if let Some(s) = duration {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(s));
                    h.stop();
                });
            }
        });
        // Whatever happened, close the store: flush + clean-shutdown mark.
        let fin = collector.finish();
        res?;
        fin?;
        Ok(0)
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(app::main());
}
