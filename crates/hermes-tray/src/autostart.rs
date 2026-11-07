//! "Start with Windows": a per-user value under HKCU\...\Run. Off by default, toggled only by the user from
//! the tray menu, listed by Windows in Settings > Apps > Startup and Task Manager > Startup apps.
#![allow(unsafe_code)]

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const VALUE: PCWSTR = w!("Hermes Collector");
const SUBKEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

fn open() -> Result<HKEY, String> {
    let mut key = HKEY::default();
    let rc: WIN32_ERROR = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, SUBKEY, None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE | KEY_QUERY_VALUE, None, &mut key, None) };
    if rc == ERROR_SUCCESS {
        Ok(key)
    } else {
        Err(format!("cannot open the Run key (error {})", rc.0))
    }
}

pub fn is_enabled() -> bool {
    is_enabled_named(VALUE)
}

pub fn set(enabled: bool, command: &str) -> Result<(), String> {
    set_named(VALUE, enabled, command)
}

fn is_enabled_named(name: PCWSTR) -> bool {
    let Ok(key) = open() else { return false };
    let rc = unsafe { RegQueryValueExW(key, name, None, None, None, None) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    rc == ERROR_SUCCESS
}

fn set_named(name: PCWSTR, enabled: bool, command: &str) -> Result<(), String> {
    let key = open()?;
    let rc = unsafe {
        if enabled {
            let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(key, name, None, REG_SZ, Some(bytes))
        } else {
            let r = RegDeleteValueW(key, name);
            if r.0 == 2 { ERROR_SUCCESS } else { r } // 2 = already absent
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("registry error {}", rc.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses its own value name: never touches the user's real "Hermes Collector" startup entry.
    #[test]
    fn enable_disable_roundtrip_and_idempotence() {
        let name = w!("Hermes Collector TEST");
        let _ = set_named(name, false, "");
        assert!(!is_enabled_named(name));
        set_named(name, true, "\"C:\\Program Files\\Hermes\\hermes-tray.exe\" --data-dir \"C:\\d\"").unwrap();
        assert!(is_enabled_named(name));
        set_named(name, true, "\"C:\\x.exe\"").unwrap(); // overwrite is fine
        assert!(is_enabled_named(name));
        set_named(name, false, "").unwrap();
        assert!(!is_enabled_named(name));
        set_named(name, false, "").unwrap(); // already absent: not an error
    }
}
