//! Links the VC++ runtime statically on the MSVC target, and leaves the UCRT
//! dynamic, as Windows 10 and 11 ship it: the installer's hooks run the CLI
//! to edit the PATH and to list parked Entries, on a Windows that may have no
//! VC++ runtime installed.

fn main() {
    static_vcruntime::metabuild();
}
