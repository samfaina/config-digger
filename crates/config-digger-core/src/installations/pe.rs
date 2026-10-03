//! The product version in a Windows executable's version resource, read
//! from its bytes in parts: the headers, then the resource section.

use crate::ports::FileSystem;
use std::path::Path;

/// `RT_VERSION`, the resource type of a version resource.
const RT_VERSION: u32 = 16;
/// What `VS_FIXEDFILEINFO` starts with.
const FIXED_INFO_SIGNATURE: u32 = 0xFEEF_04BD;
/// How much of the file holds the headers and section table.
const HEADERS: usize = 4096;
/// The largest resource section read, icons included.
const MAX_RESOURCES: usize = 64 * 1024 * 1024;

/// The product version of the executable at `path`, as its four numeric
/// components; `None` when it has none or is not an executable.
pub(crate) fn product_version(fs: &dyn FileSystem, path: &Path) -> Option<[u16; 4]> {
    let headers = fs.read_at(path, 0, HEADERS).ok()?;
    if headers.get(..2)? != b"MZ" {
        return None;
    }
    let pe = to_usize(u32_at(&headers, 0x3C)?)?;
    if headers.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = usize::from(u16_at(&headers, coff + 2)?);
    let optional = coff + 20;
    let optional_size = usize::from(u16_at(&headers, coff + 16)?);
    let (count_at, directories_at) = match u16_at(&headers, optional)? {
        0x10B => (92, 96),
        0x20B => (108, 112),
        _ => return None,
    };
    if u32_at(&headers, optional + count_at)? <= 2 {
        return None;
    }
    let resources_rva = u32_at(&headers, optional + directories_at + 16)?;
    if resources_rva == 0 {
        return None;
    }
    // The section that holds the resource directory.
    let table = optional + optional_size;
    let (section_rva, raw) = (0..sections).find_map(|index| {
        let header = table + index * 40;
        let virtual_size = u32_at(&headers, header + 8)?;
        let rva = u32_at(&headers, header + 12)?;
        let raw_size = u32_at(&headers, header + 16)?;
        let raw_offset = u32_at(&headers, header + 20)?;
        let size = virtual_size.max(raw_size);
        (rva <= resources_rva && resources_rva < rva.checked_add(size)?)
            .then_some((rva, (raw_offset, raw_size)))
    })?;
    let section = fs
        .read_at(path, u64::from(raw.0), to_usize(raw.1)?.min(MAX_RESOURCES))
        .ok()?;
    let root = to_usize(resources_rva - section_rva)?;

    // Type RT_VERSION, then its first name, then its first language.
    let named = find_entry(&section, root, Some(RT_VERSION))?;
    let languages = find_entry(&section, root + subdirectory(named)?, None)?;
    let data = find_entry(&section, root + subdirectory(languages)?, None)?;
    if data & 0x8000_0000 != 0 {
        return None;
    }
    let data = root + to_usize(data)?;
    let info_rva = u32_at(&section, data)?;
    let info_size = to_usize(u32_at(&section, data + 4)?)?;
    let info_at = to_usize(info_rva.checked_sub(section_rva)?)?;
    let info = section.get(info_at..info_at.checked_add(info_size)?)?;
    fixed_product_version(info)
}

/// The product version in `VS_VERSIONINFO`, whose value is a
/// `VS_FIXEDFILEINFO` after its key and padding to 32 bits.
fn fixed_product_version(info: &[u8]) -> Option<[u16; 4]> {
    let value_length = usize::from(u16_at(info, 2)?);
    if value_length < 52 {
        return None;
    }
    // The key, `VS_VERSION_INFO`, ends with a UTF-16 null.
    let mut at = 6;
    while u16_at(info, at)? != 0 {
        at += 2;
    }
    let value = (at + 2).next_multiple_of(4);
    if u32_at(info, value)? != FIXED_INFO_SIGNATURE {
        return None;
    }
    let high = u32_at(info, value + 16)?;
    let low = u32_at(info, value + 20)?;
    let [a, b] = halves(high);
    let [c, d] = halves(low);
    Some([a, b, c, d])
}

/// The target of the entry with id `id`, or the first entry when `id` is
/// `None`, of the resource directory at `at`.
fn find_entry(section: &[u8], at: usize, id: Option<u32>) -> Option<u32> {
    let named = usize::from(u16_at(section, at + 12)?);
    let ids = usize::from(u16_at(section, at + 14)?);
    (0..named + ids).find_map(|index| {
        let entry = at + 16 + index * 8;
        let name = u32_at(section, entry)?;
        let matches = match id {
            Some(id) => index >= named && name == id,
            None => true,
        };
        matches.then(|| u32_at(section, entry + 4)).flatten()
    })
}

/// The offset of the subdirectory an entry points to.
fn subdirectory(target: u32) -> Option<usize> {
    (target & 0x8000_0000 != 0).then(|| to_usize(target & 0x7FFF_FFFF))?
}

/// The high and low 16 bits of `value`.
fn halves(value: u32) -> [u16; 2] {
    [(value >> 16) as u16, (value & 0xFFFF) as u16]
}

/// The little-endian `u16` at `at`, `None` past the end of `bytes`.
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

/// The little-endian `u32` at `at`, `None` past the end of `bytes`.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// `value` as an offset or size.
fn to_usize(value: u32) -> Option<usize> {
    usize::try_from(value).ok()
}
