//! Windows volume probe. The main crate forbids unsafe code, so the Win32
//! calls live here. Other platforms do not call this crate.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVolume {
    pub kind: String,
    pub read_only: bool,
    pub remote: bool,
}

#[cfg(windows)]
const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;
#[cfg(windows)]
const DRIVE_REMOTE: u32 = 4;

/// Name the filesystem of an existing path. A remote drive stays remote even
/// when Windows reports the server's filesystem name, often NTFS.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn probe(path: &Path) -> std::io::Result<WindowsVolume> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW,
    };

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.len() >= 32_767 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "volume path length is not usable",
        ));
    }
    wide.push(0);
    let mut root = vec![0u16; 32_768];
    let found = unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) };
    if found == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let root_len = root.iter().position(|unit| *unit == 0).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "volume root was not terminated",
        )
    })?;
    if root_len == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "volume root was empty",
        ));
    }
    let mut name = [0u16; 64];
    let mut flags = 0u32;
    let described = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if described == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let name_len = name.iter().position(|unit| *unit == 0).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "filesystem name was not terminated",
        )
    })?;
    let kind = std::ffi::OsString::from_wide(&name[..name_len])
        .to_string_lossy()
        .trim()
        .to_ascii_lowercase();
    if kind.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "volume returned an empty filesystem name",
        ));
    }
    let drive = unsafe { GetDriveTypeW(root.as_ptr()) };
    Ok(WindowsVolume {
        kind,
        read_only: flags & FILE_READ_ONLY_VOLUME != 0,
        remote: drive == DRIVE_REMOTE,
    })
}

#[cfg(not(windows))]
pub fn probe(path: &Path) -> std::io::Result<WindowsVolume> {
    let _ = path;
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the volume probe is only built on windows",
    ))
}
