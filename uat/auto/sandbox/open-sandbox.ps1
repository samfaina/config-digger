# Opens one Windows Sandbox on a .wsb run.ps1 wrote, waits for the done.txt
# its in-sandbox.ps1 writes in -Results, and waits for the Sandbox to close,
# since only one runs at a time.
param(
    [Parameter(Mandatory)] [string]$Wsb,
    [Parameter(Mandatory)] [string]$Results,
    [int]$TimeoutMinutes = 40
)
$ErrorActionPreference = "Stop"

function WaitSandboxClosed([int]$seconds = 180) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Get-Process -Name WindowsSandbox* -ErrorAction SilentlyContinue) -and (Get-Date) -lt $end) { Start-Sleep -Seconds 3 }
}
$run = [IO.Path]::GetFileNameWithoutExtension($Wsb)
WaitSandboxClosed
Write-Host "$run`: opening Windows Sandbox"
Start-Process $Wsb
$end = (Get-Date).AddMinutes($TimeoutMinutes)
while (-not (Test-Path "$Results\done.txt") -and (Get-Date) -lt $end) { Start-Sleep -Seconds 10 }
if (-not (Test-Path "$Results\done.txt")) { Write-Warning "$run`: no done.txt after $TimeoutMinutes minutes" }
WaitSandboxClosed
