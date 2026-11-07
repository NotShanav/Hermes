//! Single-instance guard: only one process may write a given data directory (collector, tray or the
//! re-encryption tool). A named mutex, scoped to the logon session, is released by the OS when the
//! process exits or crashes, so there is no stale lock file to clean up.

use std::path::Path;

/// Mutex name for a data directory (case-insensitive path, FNV-1a hash).
pub fn instance_name(data_dir: &Path) -> String {
    let norm = data_dir.to_string_lossy().to_lowercase().replace('/', "\\");
    let norm = norm.trim_end_matches('\\');
    let mut h: u64 = 0xcbf29ce484222325;
    for b in norm.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("Local\\HermesDataDir-{h:016x}")
}

pub struct SingleInstance {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    /// `Ok(Some(guard))` = we are the only instance; `Ok(None)` = another process already owns this data dir.
    pub fn acquire(data_dir: &Path) -> Result<Option<SingleInstance>, String> {
        let name: Vec<u16> = instance_name(data_dir).encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let h = CreateMutexW(None, false, PCWSTR(name.as_ptr())).map_err(|e| format!("CreateMutexW: {e}"))?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(h);
                return Ok(None);
            }
            Ok(Some(SingleInstance { handle: h }))
        }
    }

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(windows)]
pub use imp::acquire;

/// Non-Windows builds (development/testing only): no OS-wide guard.
#[cfg(not(windows))]
pub fn acquire(_data_dir: &Path) -> Result<Option<SingleInstance>, String> {
    Ok(Some(SingleInstance {}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable_case_insensitive_and_distinct() {
        let a = instance_name(Path::new("C:\\Users\\Me\\AppData\\Local\\Hermes"));
        assert_eq!(a, instance_name(Path::new("c:\\users\\me\\appdata\\local\\hermes\\")));
        assert_ne!(a, instance_name(Path::new("C:\\Users\\Me\\AppData\\Local\\Hermes-test")));
        assert!(a.starts_with("Local\\HermesDataDir-"));
    }

    #[cfg(windows)]
    #[test]
    fn second_acquire_is_refused_until_the_first_is_dropped() {
        let dir = Path::new("C:\\hermes-single-instance-test");
        let first = acquire(dir).unwrap().expect("first instance");
        assert!(acquire(dir).unwrap().is_none(), "second instance must be refused");
        drop(first);
        assert!(acquire(dir).unwrap().is_some(), "released after drop");
    }
}
