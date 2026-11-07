//! Volume information via GetVolumePathNameW / GetVolumeInformationW / GetDriveTypeW.
#![allow(unsafe_code)]

use crate::FsInfo;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW,
};

pub fn fs_info(path: &std::path::Path) -> Option<FsInfo> {
    let abs = std::fs::canonicalize(path).ok()?;
    let wide: Vec<u16> = abs
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut root = [0u16; 260];
        GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root).ok()?;
        let mut fs = [0u16; 64];
        let mut max_comp = 0u32;
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            None,
            Some(&mut max_comp),
            None,
            Some(&mut fs),
        )
        .ok()?;
        let to_s = |b: &[u16]| {
            String::from_utf16_lossy(&b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())])
        };
        let drive_type = match GetDriveTypeW(PCWSTR(root.as_ptr())) {
            2 => "removable",
            3 => "fixed",
            4 => "network",
            5 => "cd-rom",
            6 => "ram-disk",
            _ => "unknown",
        };
        Some(FsInfo {
            volume_root: to_s(&root),
            fs_name: to_s(&fs),
            drive_type: drive_type.into(),
            max_component_len: max_comp,
        })
    }
}
