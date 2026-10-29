# Runs the probes that write machine-wide paths, %ProgramFiles%,
# %ProgramData% and HKLM, in Windows Sandbox, where nothing they write
# reaches this machine:
#
#   .\uat\agents\sandbox.ps1
#   .\uat\agents\sandbox.ps1 -Probes codex-machine -KeepOpen
#   .\uat\agents\sandbox.ps1 -Probes claude-code-machine -Claude a\claude.exe, b\claude.exe
#   .\uat\agents\sandbox.ps1 -Probes claude-desktop -SignedIn -KeepOpen
#
# Maps read-only into the Sandbox the probes, this machine's Node, a copy of
# each Claude Code binary -Claude names, Codex's codex.exe, and the Claude
# Desktop installers claude.ai serves, downloaded into -Work. The Sandbox's
# LogonCommand runs in-sandbox.ps1, which runs each probe with
# CD_PROBE_VERBOSE, claude-code-machine once per Claude Code binary, and
# writes the output and results to -Work\results: <probe>.log and
# <probe>.json, with the Claude Code version after the probe's name. Waits
# for results\done.txt and for the Sandbox to close; -KeepOpen leaves it
# open to look around, and runs the probes again from -Work\probes, which a
# rerun may copy over, whenever a results\rerun.txt appears. -SignedIn runs
# claude-desktop's signed-in checks: its log says when to sign in from the
# Sandbox's window. Needs Windows Sandbox (Pro, Enterprise or Education).
param(
    [string]$Work = "$env:TEMP\cd-agents-sandbox",
    [ValidateSet("claude-code-machine", "codex-machine", "claude-desktop")]
    [string[]]$Probes = @("claude-code-machine", "codex-machine", "claude-desktop"),
    [string[]]$Claude = @((Get-Command claude.exe -ErrorAction SilentlyContinue).Source),
    [string]$Codex = "",
    [string]$Node = "",
    [switch]$KeepOpen,
    [switch]$SignedIn,
    [int]$TimeoutMinutes = 40
)
$ErrorActionPreference = "Stop"

# Volta's node.exe and codex.cmd are shims: map the binaries they run.
if (-not $Node) { $Node = Split-Path (& node -p "process.execPath") }
if (-not $Codex) {
    $Codex = Get-ChildItem "$env:LOCALAPPDATA\Volta\tools\image\packages\@openai\codex" -Recurse -Filter codex.exe |
        Where-Object { $_.Directory.Name -eq "bin" } | Select-Object -First 1 -ExpandProperty FullName
}
foreach ($file in @("$Node\node.exe", $Codex) + $Claude) {
    if (-not $file -or -not (Test-Path $file)) { throw "not found: $file" }
}

$results = "$Work\results"
$desktop = "$Work\desktop"
$probesDir = "$Work\probes"
$claudes = "$Work\claude"
foreach ($dir in $results, $probesDir, $claudes) { if (Test-Path $dir) { Remove-Item $dir -Recurse -Force } }
New-Item -ItemType Directory -Force $Work, $results, $desktop, $probesDir, $claudes | Out-Null

# Each Claude Code binary as <version>\claude.exe: those Claude Code keeps
# in ~\.local\share\claude\versions have no extension.
foreach ($file in $Claude) {
    $version = (Get-Item $file).VersionInfo.ProductVersion -replace '\.0$', ''
    New-Item -ItemType Directory -Force "$claudes\$version" | Out-Null
    Copy-Item $file "$claudes\$version\claude.exe"
}

# The installers claude.ai serves, as its download page links them.
if ($Probes -contains "claude-desktop") {
    foreach ($kind in @(@("exe", "claude-exe.exe"), @("msix", "claude-msix.msix"))) {
        if (-not (Test-Path "$desktop\$($kind[1])")) {
            Invoke-WebRequest "https://claude.ai/api/desktop/win32/x64/$($kind[0])/latest/redirect" -OutFile "$desktop\$($kind[1])" -UseBasicParsing
        }
    }
}

# The probes, and in-sandbox.ps1 with a byte order mark so that Windows
# PowerShell 5.1 reads it as UTF-8.
Copy-Item "$PSScriptRoot\*.mjs" $probesDir
[IO.File]::WriteAllText("$probesDir\in-sandbox.ps1", [IO.File]::ReadAllText("$PSScriptRoot\in-sandbox.ps1"), [Text.UTF8Encoding]::new($true))
if ($KeepOpen) { Set-Content "$probesDir\keep-open" "" }

function Mapped([string]$hostFolder, [string]$sandbox, [bool]$readOnly = $true) {
    "<MappedFolder><HostFolder>$hostFolder</HostFolder><SandboxFolder>$sandbox</SandboxFolder><ReadOnly>$($readOnly.ToString().ToLower())</ReadOnly></MappedFolder>"
}
function WaitSandboxClosed([int]$seconds = 180) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Get-Process -Name WindowsSandbox* -ErrorAction SilentlyContinue) -and (Get-Date) -lt $end) { Start-Sleep -Seconds 3 }
}

$folders = @(
    (Mapped $probesDir "C:\probe\agents"),
    (Mapped $Node "C:\probe\node"),
    (Mapped $claudes "C:\probe\claude"),
    (Mapped (Split-Path $Codex) "C:\probe\codex"),
    (Mapped $desktop "C:\probe\desktop"),
    (Mapped $results "C:\probe\results" $false)
)
$command = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\probe\agents\in-sandbox.ps1 -Probes $($Probes -join ',') -Codex $(Split-Path -Leaf $Codex)$(if ($SignedIn) { ' -SignedIn' })"
$wsb = "$Work\agents.wsb"
Set-Content $wsb @"
<Configuration>
  <Networking>Enable</Networking>
  <vGPU>Disable</vGPU>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <MemoryInMB>8192</MemoryInMB>
  <MappedFolders>$($folders -join '')</MappedFolders>
  <LogonCommand><Command>$command</Command></LogonCommand>
</Configuration>
"@

WaitSandboxClosed
Write-Host "opening Windows Sandbox for $($Probes -join ', ')"
Start-Process $wsb
$end = (Get-Date).AddMinutes($TimeoutMinutes)
while (-not (Test-Path "$results\done.txt") -and (Get-Date) -lt $end) { Start-Sleep -Seconds 10 }
if (-not (Test-Path "$results\done.txt")) { Write-Warning "no done.txt after $TimeoutMinutes minutes" }
if (-not $KeepOpen) { WaitSandboxClosed }
foreach ($log in Get-ChildItem "$results\*.log") {
    Write-Host "`n--- $($log.BaseName)"
    Get-Content $log.FullName -Encoding UTF8
}
Write-Host "`nresults in $results"
