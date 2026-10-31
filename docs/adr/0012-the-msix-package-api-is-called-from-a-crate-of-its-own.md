# The MSIX package API is called from a crate of its own

A user who is not an administrator cannot list `C:\Program Files\WindowsApps`, so an Installation of an MSIX package, such as Claude Desktop's, names its package family, and config-digger asks Windows for the family's packages and their folders (#469). Windows answers through its documented package API, `GetPackagesByPackageFamily` and `GetPackagePathByFullName`, which needs no elevation, and calling it takes unsafe code. config-digger's crates forbid unsafe code (ADR 0008), and no third-party crate wraps these two calls safely, so the calls live in a crate of their own, `config-digger-msix`: Windows only, two safe functions over `windows-sys`'s narrow feature, and the only crate of the workspace that allows unsafe code. Every other crate still forbids it. We rejected reading the undocumented `AppModel\Repository\Packages` registry key, asking PowerShell's `Get-AppxPackage`, which starts a process at every scan, and the `windows` crate's WinRT `PackageManager`, which is safe to call but a far larger dependency for two calls.

## Consequences

- `config-digger-msix` does not take the workspace's lints, whose only one is `unsafe_code = "forbid"`. A lint added to the workspace later has to be added to it too.
- Its unsafe code is the two calls and the buffers Windows sizes for them. It reads the full names from the buffer, never through the pointers Windows writes.
- The core reaches it only through the `Environment` port's `package_folders`, which the fake ports answer from `FakeEnvironment::with_package`.
