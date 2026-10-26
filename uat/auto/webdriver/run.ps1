# Walks the UAT steps the real app shows, through WebDriver, on a throwaway
# Windows profile and the throwaway data. Writes the results as JSON, in the
# shape of the runner's results documents, and the screenshots the agent judge
# reads, all under -Work, outside the repo. Screenshots never leave this
# machine (docs/agents/screenshots.md).
#
#   .\uat\auto\webdriver\run.ps1 -App target\release\config-digger-app.exe
#   .\uat\auto\webdriver\run.ps1 -App … -Only 11.1,11.2
#   .\uat\auto\webdriver\run.ps1 -App … -Ci -Evidence <run URL> -Summary $env:GITHUB_STEP_SUMMARY
#
# With -Ci, as the CI job runs it, the walk takes no screenshot and writes
# uat-results.json under -Work: the runner's fields, `by: "ci"` and the run's
# URL as `evidence`.
#
# Needs tauri-driver (cargo install tauri-driver --locked) and Node. Fetches
# the msedgedriver whose version is the installed WebView2's. Step 7.2 opens
# Claude Desktop when it is installed and closed, and closes it again.
param(
    [Parameter(Mandatory)][string]$App,
    [string]$Work = "$env:TEMP\config-digger-uat",
    [string]$Only,
    [string]$Root = "E:\cd-throwaway",
    [switch]$Ci,
    [string]$Evidence,
    [string]$Summary
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
if ($Ci -and -not $Evidence) { throw "-Ci needs -Evidence, the URL of the CI run" }

if (-not (Get-Command tauri-driver -ErrorAction SilentlyContinue)) { throw "tauri-driver is not on the PATH: cargo install tauri-driver --locked" }
$App = (Resolve-Path $App).Path
New-Item -ItemType Directory -Force $Work | Out-Null

# The Edge driver must match WebView2 to the build.
$webview = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}').pv
$driverDir = Join-Path $Work "msedgedriver-$webview"
$driver = Join-Path $driverDir "msedgedriver.exe"
if (-not (Test-Path $driver)) {
    $zip = Join-Path $Work "edgedriver-$webview.zip"
    try { Invoke-WebRequest "https://msedgedriver.microsoft.com/$webview/edgedriver_win64.zip" -OutFile $zip }
    catch { throw "no msedgedriver is published for WebView2 ${webview}: $($_.Exception.Message)" }
    Expand-Archive $zip $driverDir -Force
}
Write-Output "msedgedriver $webview, the WebView2 version in the EdgeUpdate registry key"

# A fresh profile each run: the app keeps its lists, settings, backups and
# log there. The name is checked, so only a throwaway folder is ever deleted.
$throwaway = Join-Path $Work "cd-throwaway-profile"
if ((Split-Path $throwaway -Leaf) -ne "cd-throwaway-profile") { throw "refusing to reset $throwaway" }
if (Test-Path $throwaway) { Remove-Item $throwaway -Recurse -Force }

$results = Join-Path $Work $(if ($Ci) { "uat-results.json" } else { "webdriver.json" })
foreach ($file in $results, ($results -replace '\.json$', '.shared.json')) { if (Test-Path $file) { Remove-Item $file } }
$shots = Join-Path $Work "shots"
if (Test-Path $shots) { Remove-Item $shots -Recurse -Force }

& "$here\..\..\data\cd-throwaway-setup.ps1" -Root $Root -Reset | Out-Null

# The version under test is the one the executable reports, whatever the
# checkout says.
$version = (Get-Item $App).VersionInfo.ProductVersion
$arguments = @("$here\walk.mjs", "--app", $App, "--version", $version, "--driver", $driver, "--profile", $throwaway, "--out", $results, "--root", $Root)
if ($Ci) { $arguments += @("--ci", "--evidence", $Evidence) } else { $arguments += @("--shots", $shots) }
if ($Summary) { $arguments += @("--summary", $Summary) }
if ($Only) { $arguments += @("--only", $Only) }
node @arguments
if ($LASTEXITCODE) { throw "the walk stopped with exit code ${LASTEXITCODE}: see its output above" }
Write-Output "Results: $results"
if (-not $Ci) { Write-Output "Screenshots, for the agent judge only: $shots" }
