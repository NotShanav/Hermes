//! Locating the USB key file and the local time zone.

use std::path::{Path, PathBuf};

pub const DEFAULT_SHARE_B_NAME: &str = "hermes-share-b.key";

/// `E:\` or `E:` or a directory -> `<dir>\hermes-share-b.key`; an explicit file name is kept.
pub fn share_b_target(usb: &str) -> PathBuf {
    let p = PathBuf::from(usb);
    if p.is_dir() {
        return p.join(DEFAULT_SHARE_B_NAME);
    }
    let bare_drive = usb.len() == 2 && usb.ends_with(':');
    if bare_drive || usb.ends_with('\\') || usb.ends_with('/') {
        // keep the user's separator style: `E:\` -> `E:\hermes-share-b.key`, `E:/` -> `E:/hermes-share-b.key`
        let sep = if bare_drive || usb.contains('\\') { '\\' } else { '/' };
        let base = usb.trim_end_matches(['\\', '/']);
        return PathBuf::from(format!("{base}{sep}{DEFAULT_SHARE_B_NAME}"));
    }
    p
}

/// Drive letter (upper case) of a Windows path, if it has one.
pub fn drive_letter(p: &Path) -> Option<char> {
    let s = p.to_string_lossy();
    let mut c = s.chars();
    match (c.next(), c.next()) {
        (Some(l), Some(':')) if l.is_ascii_alphabetic() => Some(l.to_ascii_uppercase()),
        _ => None,
    }
}

/// True when both paths are known to be on the same Windows drive (the USB share must NOT live on the laptop's disk).
pub fn same_drive(a: &Path, b: &Path) -> bool {
    matches!((drive_letter(a), drive_letter(b)), (Some(x), Some(y)) if x == y)
}

/// Key files named `hermes-share-b.key` at the root of any drive (Windows).
pub fn auto_detect() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        ('A'..='Z').map(|l| PathBuf::from(format!("{l}:\\{DEFAULT_SHARE_B_NAME}"))).filter(|p| p.is_file()).collect()
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

/// Local UTC offset in minutes (Windows time-zone settings; 0 elsewhere).
pub fn local_offset_minutes() -> i32 {
    #[cfg(windows)]
    {
        use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz = TIME_ZONE_INFORMATION::default();
        let id = unsafe { GetTimeZoneInformation(&mut tz) };
        let bias = tz.Bias + match id {
            1 => tz.StandardBias,
            2 => tz.DaylightBias,
            _ => 0,
        };
        -bias
    }
    #[cfg(not(windows))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_b_target_handles_drive_dir_and_file_forms() {
        assert_eq!(share_b_target("E:\\"), PathBuf::from("E:\\hermes-share-b.key"));
        assert_eq!(share_b_target("E:/"), PathBuf::from("E:/hermes-share-b.key"));
        assert!(share_b_target("E:").to_string_lossy().ends_with("hermes-share-b.key"));
        assert_eq!(share_b_target("E:\\keys\\mine.key"), PathBuf::from("E:\\keys\\mine.key"));
        let d = tempfile::tempdir().unwrap();
        assert_eq!(share_b_target(d.path().to_str().unwrap()), d.path().join("hermes-share-b.key"));
    }

    #[test]
    fn same_drive_detection() {
        assert!(same_drive(Path::new("C:\\Users\\a\\Hermes"), Path::new("c:\\stick.key")));
        assert!(!same_drive(Path::new("C:\\Users\\a\\Hermes"), Path::new("E:\\hermes-share-b.key")));
        assert!(!same_drive(Path::new("/tmp/a"), Path::new("/tmp/b")), "not checkable off Windows");
    }
}
