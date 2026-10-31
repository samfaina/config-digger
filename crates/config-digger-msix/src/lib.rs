//! The MSIX packages installed for the current user, asked of Windows's
//! documented package API, which needs no elevation: never by listing
//! `C:\Program Files\WindowsApps`, which only an administrator can list.
//! The unsafe code the Win32 calls take stays in this crate, since the core
//! forbids it. Windows only.

#![cfg(windows)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::Storage::Packaging::Appx::{
    GetPackagePathByFullName, GetPackagesByPackageFamily,
};

/// How many times the full names are asked again when a package is
/// installed between the call that sizes the buffers and the one that
/// fills them.
const ATTEMPTS: usize = 3;

/// The install folder of each package of the package family `family`, such
/// as `Claude_pzs8sxrjxfjjc`, installed for the current user, in the order
/// Windows gives them: none when the family has none installed. An error
/// when Windows cannot be asked.
pub fn family_folders(family: &str) -> io::Result<Vec<PathBuf>> {
    full_names(family)?
        .iter()
        .map(|full_name| package_path(full_name))
        .collect()
}

/// `text` as a null-terminated UTF-16 string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn error(code: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(code as i32)
}

/// The full names of the family's packages, each null-terminated.
fn full_names(family: &str) -> io::Result<Vec<Vec<u16>>> {
    let family = wide(family);
    for _ in 0..ATTEMPTS {
        let mut count = 0u32;
        let mut length = 0u32;
        // SAFETY: `family` is null-terminated, and null buffers with a
        // count and length of 0 ask only for their sizes.
        let sized = unsafe {
            GetPackagesByPackageFamily(
                family.as_ptr(),
                &mut count,
                std::ptr::null_mut(),
                &mut length,
                std::ptr::null_mut(),
            )
        };
        match sized {
            ERROR_SUCCESS if count == 0 => return Ok(Vec::new()),
            ERROR_SUCCESS | ERROR_INSUFFICIENT_BUFFER => {}
            code => return Err(error(code)),
        }
        let mut pointers = vec![std::ptr::null_mut(); count as usize];
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: `pointers` holds `count` items and `buffer` `length`
        // characters, as Windows asked for; the pointers it writes point
        // into `buffer` and are never read.
        let filled = unsafe {
            GetPackagesByPackageFamily(
                family.as_ptr(),
                &mut count,
                pointers.as_mut_ptr(),
                &mut length,
                buffer.as_mut_ptr(),
            )
        };
        match filled {
            // The buffer holds the full names one after another, each
            // ended by a null.
            ERROR_SUCCESS => {
                return Ok(buffer
                    .split_inclusive(|&c| c == 0)
                    .take(count as usize)
                    .map(<[u16]>::to_vec)
                    .collect());
            }
            ERROR_INSUFFICIENT_BUFFER => continue,
            code => return Err(error(code)),
        }
    }
    Err(error(ERROR_INSUFFICIENT_BUFFER))
}

/// The install folder of the package with the null-terminated full name
/// `full_name`.
fn package_path(full_name: &[u16]) -> io::Result<PathBuf> {
    let mut length = 0u32;
    // SAFETY: `full_name` is null-terminated, and a null buffer with a
    // length of 0 asks only for its size, which no path fits in.
    let sized =
        unsafe { GetPackagePathByFullName(full_name.as_ptr(), &mut length, std::ptr::null_mut()) };
    if sized != ERROR_INSUFFICIENT_BUFFER {
        return Err(error(sized));
    }
    let mut path = vec![0u16; length as usize];
    // SAFETY: `path` holds `length` characters, as Windows asked for.
    let filled =
        unsafe { GetPackagePathByFullName(full_name.as_ptr(), &mut length, path.as_mut_ptr()) };
    if filled != ERROR_SUCCESS {
        return Err(error(filled));
    }
    let end = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    Ok(PathBuf::from(OsString::from_wide(&path[..end])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_family_with_no_package_installed_has_no_folders() {
        assert_eq!(
            family_folders("ConfigDigger.NoSuchPackage_0000000000000").unwrap(),
            Vec::<PathBuf>::new()
        );
    }
}
