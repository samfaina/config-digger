//! The DLLs an executable imports, read from its PE import table and checked
//! against the VC++ runtime's: a Windows without the VC++ runtime installed
//! cannot start an executable that imports one, and says nothing.

/// The VC++ runtime's DLLs, in lowercase. The UCRT's (`api-ms-win-crt-*`,
/// `ucrtbase.dll`) are not among them: Windows 10 and 11 ship it.
const VC_RUNTIME: [&str; 3] = ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"];

/// What is wrong with the executable `exe`: each VC++ runtime DLL it
/// imports, or that it cannot be read as a PE executable.
pub fn problems(exe: &[u8]) -> Vec<String> {
    match imports(exe) {
        Ok(dlls) => dlls
            .into_iter()
            .filter(|dll| VC_RUNTIME.contains(&dll.to_lowercase().as_str()))
            .map(|dll| format!("it imports {dll}, which a Windows without the VC++ runtime lacks"))
            .collect(),
        Err(error) => vec![format!("it is no PE executable: {error}")],
    }
}

// Where the PE format puts what the check reads, as offsets from the start
// of the file, of the PE signature, of the optional header, of a section
// header and of an import descriptor.
const PE_OFFSET: usize = 0x3C;
const SECTION_COUNT: usize = 6;
const OPTIONAL_HEADER_SIZE: usize = 20;
const OPTIONAL_HEADER: usize = 24;
/// PE32+, the optional header of a 64-bit executable.
const PE32_PLUS: u16 = 0x20B;
const DIRECTORY_COUNT: usize = 108;
const IMPORT_DIRECTORY: usize = 120;
const SECTION_HEADER_SIZE: usize = 40;
const SECTION_ADDRESS: usize = 12;
const SECTION_DATA_SIZE: usize = 16;
const SECTION_DATA: usize = 20;
const DESCRIPTOR_SIZE: usize = 20;
const DESCRIPTOR_NAME: usize = 12;

/// The DLLs the import table of `exe` names, in its order.
fn imports(exe: &[u8]) -> Result<Vec<String>, String> {
    if exe.get(..2) != Some(b"MZ") {
        return Err("no MZ header".to_owned());
    }
    let pe = u32_at(exe, PE_OFFSET)? as usize;
    if exe.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err("no PE signature".to_owned());
    }
    let optional_header = pe + OPTIONAL_HEADER;
    match u16_at(exe, optional_header)? {
        PE32_PLUS => {}
        magic => return Err(format!("the optional header {magic:#x} is not PE32+'s")),
    }
    // The import table is the second data directory.
    if u32_at(exe, optional_header + DIRECTORY_COUNT)? < 2 {
        return Ok(Vec::new());
    }
    let import_table = u32_at(exe, optional_header + IMPORT_DIRECTORY)?;
    if import_table == 0 {
        return Ok(Vec::new());
    }
    let sections = Sections {
        exe,
        headers: optional_header + u16_at(exe, pe + OPTIONAL_HEADER_SIZE)? as usize,
        count: u16_at(exe, pe + SECTION_COUNT)? as usize,
    };
    let mut dlls = Vec::new();
    let mut descriptor = sections.file_offset(import_table)?;
    loop {
        let name_address = u32_at(exe, descriptor + DESCRIPTOR_NAME)?;
        if name_address == 0 {
            return Ok(dlls);
        }
        let name = sections.file_offset(name_address)?;
        let length = exe
            .get(name..)
            .and_then(|rest| rest.iter().position(|&byte| byte == 0))
            .ok_or_else(|| format!("the file ends before the DLL name at {name:#x}"))?;
        dlls.push(String::from_utf8_lossy(&exe[name..name + length]).into_owned());
        descriptor += DESCRIPTOR_SIZE;
    }
}

/// The section headers of an executable.
struct Sections<'a> {
    exe: &'a [u8],
    /// Where the first one starts in the file.
    headers: usize,
    count: usize,
}

impl Sections<'_> {
    /// Where the address `rva` lies in the file: in the data of the section
    /// that holds it.
    fn file_offset(&self, rva: u32) -> Result<usize, String> {
        for index in 0..self.count {
            let header = self.headers + SECTION_HEADER_SIZE * index;
            let address = u32_at(self.exe, header + SECTION_ADDRESS)?;
            let size = u32_at(self.exe, header + SECTION_DATA_SIZE)?;
            let data = u32_at(self.exe, header + SECTION_DATA)?;
            if rva >= address && rva - address < size {
                return Ok(data as usize + (rva - address) as usize);
            }
        }
        Err(format!("no section holds the address {rva:#x} in the file"))
    }
}

fn u16_at(exe: &[u8], offset: usize) -> Result<u16, String> {
    bytes_at(exe, offset).map(u16::from_le_bytes)
}

fn u32_at(exe: &[u8], offset: usize) -> Result<u32, String> {
    bytes_at(exe, offset).map(u32::from_le_bytes)
}

fn bytes_at<const N: usize>(exe: &[u8], offset: usize) -> Result<[u8; N], String> {
    exe.get(offset..offset + N)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| format!("the file ends before the offset {offset:#x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PE32+ executable whose import table names `dlls`, in one section.
    fn importing(dlls: &[&str]) -> Vec<u8> {
        const LFANEW: usize = 0x40;
        const OPTIONAL_HEADER: usize = LFANEW + 24;
        const OPTIONAL_HEADER_SIZE: usize = 240;
        const SECTIONS: usize = OPTIONAL_HEADER + OPTIONAL_HEADER_SIZE;
        const RAW: usize = 0x200;
        const VIRTUAL: usize = 0x1000;

        // The section: the import descriptors, a zero one last, then the names.
        let descriptors = 20 * (dlls.len() + 1);
        let mut section = vec![0; descriptors];
        for (index, dll) in dlls.iter().enumerate() {
            let name = (VIRTUAL + section.len()) as u32;
            section[20 * index + 12..20 * index + 16].copy_from_slice(&name.to_le_bytes());
            section.extend_from_slice(dll.as_bytes());
            section.push(0);
        }

        let mut exe = vec![0; RAW];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3C..0x40].copy_from_slice(&(LFANEW as u32).to_le_bytes());
        exe[LFANEW..LFANEW + 4].copy_from_slice(b"PE\0\0");
        // The COFF header: x86_64, one section, and the optional header's size.
        exe[LFANEW + 4..LFANEW + 6].copy_from_slice(&0x8664u16.to_le_bytes());
        exe[LFANEW + 6..LFANEW + 8].copy_from_slice(&1u16.to_le_bytes());
        exe[LFANEW + 20..LFANEW + 22].copy_from_slice(&(OPTIONAL_HEADER_SIZE as u16).to_le_bytes());
        // The optional header: PE32+, 16 data directories, the second the imports.
        exe[OPTIONAL_HEADER..OPTIONAL_HEADER + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        exe[OPTIONAL_HEADER + 108..OPTIONAL_HEADER + 112].copy_from_slice(&16u32.to_le_bytes());
        exe[OPTIONAL_HEADER + 120..OPTIONAL_HEADER + 124]
            .copy_from_slice(&(VIRTUAL as u32).to_le_bytes());
        exe[OPTIONAL_HEADER + 124..OPTIONAL_HEADER + 128]
            .copy_from_slice(&(descriptors as u32).to_le_bytes());
        // The section header.
        let header = &mut exe[SECTIONS..SECTIONS + 40];
        header[..6].copy_from_slice(b".idata");
        header[8..12].copy_from_slice(&(section.len() as u32).to_le_bytes());
        header[12..16].copy_from_slice(&(VIRTUAL as u32).to_le_bytes());
        header[16..20].copy_from_slice(&(section.len() as u32).to_le_bytes());
        header[20..24].copy_from_slice(&(RAW as u32).to_le_bytes());

        exe.extend_from_slice(&section);
        exe
    }

    #[test]
    fn importing_the_vc_runtime_is_a_problem() {
        let exe = importing(&["KERNEL32.dll", "VCRUNTIME140.dll", "ntdll.dll"]);

        assert_eq!(
            problems(&exe),
            vec!["it imports VCRUNTIME140.dll, which a Windows without the VC++ runtime lacks"]
        );
    }

    #[test]
    fn the_ucrt_and_windows_dlls_are_no_problem() {
        let exe = importing(&[
            "KERNEL32.dll",
            "api-ms-win-crt-runtime-l1-1-0.dll",
            "api-ms-win-crt-heap-l1-1-0.dll",
            "ucrtbase.dll",
        ]);

        assert_eq!(problems(&exe), Vec::<String>::new());
    }

    #[test]
    fn each_vc_runtime_dll_is_a_problem_whatever_its_case() {
        let exe = importing(&["vcruntime140_1.dll", "MSVCP140.dll", "VCRuntime140.DLL"]);

        assert_eq!(
            problems(&exe),
            vec![
                "it imports vcruntime140_1.dll, which a Windows without the VC++ runtime lacks",
                "it imports MSVCP140.dll, which a Windows without the VC++ runtime lacks",
                "it imports VCRuntime140.DLL, which a Windows without the VC++ runtime lacks",
            ]
        );
    }

    #[test]
    fn a_file_that_is_no_executable_is_a_problem() {
        assert_eq!(
            problems(b"#!/bin/sh\n"),
            vec!["it is no PE executable: no MZ header"]
        );
    }

    #[test]
    fn an_executable_cut_in_its_section_headers_is_a_problem() {
        let exe = importing(&["VCRUNTIME140.dll"]);

        assert_eq!(
            problems(&exe[..0x150]),
            vec!["it is no PE executable: the file ends before the offset 0x154"]
        );
    }

    #[test]
    fn an_executable_cut_before_a_dll_name_is_a_problem() {
        let exe = importing(&["VCRUNTIME140.dll"]);

        assert_eq!(
            problems(&exe[..0x214]),
            vec!["it is no PE executable: the file ends before the DLL name at 0x228"]
        );
    }

    #[test]
    fn an_address_past_its_section_data_in_the_file_is_a_problem() {
        let mut exe = importing(&["VCRUNTIME140.dll"]);
        // The section's data in the file holds the descriptors only: the
        // name lies in the part the loader fills with zeros.
        exe[0x148 + 16..0x148 + 20].copy_from_slice(&40u32.to_le_bytes());

        assert_eq!(
            problems(&exe),
            vec!["it is no PE executable: no section holds the address 0x1028 in the file"]
        );
    }
}
