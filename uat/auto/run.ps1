# The local script: the UAT steps that need the tester's PC, in one command,
# before each release candidate (docs/agents/uat.md, "The local script").
# It runs, in order:
#
# 1. sandbox\build-update.ps1, the two installers 12.2 updates between, built
#    from the release candidate's tag;
# 2. sandbox\run.ps1, the Sandbox walk: 8.1, 9.1 to 9.5, 12.2 and 13.2 to 13.4;
# 3. webdriver\run.ps1, the WebDriver walk on the release build -App, with
#    7.2 and 13.1 with Claude Desktop open, and 14.2 on the tester's own homes.
#
# It stops at the first one that fails, saying which. Then it writes one
# uat-results.json in -Work, every step's result from both walks in the
# runner's shape, for the single ArtifactData batch of "Publishing a run".
# The agent judge runs after it, on the walk's screenshots in -Work\shots,
# and its merge writes its verdicts into that file too.
#
#   .\uat\auto\run.ps1 -Version 0.1.0-rc.4 -App target\release\config-digger-app.exe
#
# It writes nothing outside -Root, the throwaway profile and -Work, and a
# check at the end compares config-digger's real data, the real Codex and
# Claude Desktop configuration and the checkout's working tree with how they
# were before it ran.
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$App,
    [string]$Work = "$env:TEMP\config-digger-uat",
    [string]$Root = "E:\cd-throwaway"
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$repo = (Resolve-Path "$here\..\..").Path

$App = (Resolve-Path $App).Path
$appVersion = (Get-Item $App).VersionInfo.ProductVersion
if ($appVersion -ne $Version) { throw "$App reports version $appVersion, not ${Version}: pass the release build of $Version as -App" }

New-Item -ItemType Directory -Force $Work | Out-Null
$Work = (Resolve-Path $Work).Path
$results = Join-Path $Work "uat-results.json"
if (Test-Path $results) { Remove-Item $results }
$before = Join-Path $Work "untouched.json"
node "$here\untouched.mjs" snapshot --checkout $repo --out $before
if ($LASTEXITCODE) { throw "the snapshot of the tester's own data failed" }

# Each tool, by the name the message gives it. A tool fails when it throws
# or exits with a code.
function Tool([string]$name, [string]$script, [string[]]$arguments, [string]$skipped) {
    Write-Output "== $name"
    $global:LASTEXITCODE = 0
    try { & $script @arguments } catch { throw "$name failed: $($_.Exception.Message). $skipped" }
    if ($LASTEXITCODE) { throw "$name failed with exit code ${LASTEXITCODE}: see its output above. $skipped" }
}
try {
    $update = Join-Path $Work "update"
    Tool "build-update.ps1" "$here\sandbox\build-update.ps1" @("-Commit", "v$Version", "-Work", $update) "The Sandbox walk and the WebDriver walk did not run."
    Tool "The Sandbox walk" "$here\sandbox\run.ps1" @("-Version", $Version, "-Work", (Join-Path $Work "sandbox"), "-Update", (Join-Path $update "installers")) "The WebDriver walk did not run."
    Tool "The WebDriver walk" "$here\webdriver\run.ps1" @("-App", $App, "-Work", $Work, "-Root", $Root) ""
    node "$here\results.mjs" --work $Work
    if ($LASTEXITCODE) { throw "writing $results failed: see its output above" }
} finally {
    node "$here\untouched.mjs" check --before $before --where "$Root, the throwaway profile and $Work"
    $changed = $LASTEXITCODE
}
if ($changed) { throw "the run changed the tester's own data: see the list above" }
Write-Output "Judge the walk's screenshots, then publish $results`: docs/agents/uat.md, The agent judge and Publishing a run."
