# Runs inside Windows Sandbox, from the LogonCommand of the .wsb that
# sandbox.ps1 writes: runs each probe with the mapped Node, claude-code-machine
# once per C:\probe\claude\<version>\claude.exe, and writes the output to
# C:\probe\results\<probe>.log and the results to <probe>.json, the Claude
# Code version after the probe's name, then done.txt. Shuts the Sandbox down
# unless sandbox.ps1 was given -KeepOpen; then, until the Sandbox is closed,
# it runs the probes again whenever C:\probe\results\rerun.txt appears, and
# deletes it. -SignedIn
# runs claude-desktop with --signed-in.
param([string]$Probes, [string]$Codex = "codex.exe", [switch]$SignedIn)
$results = "C:\probe\results"
$env:CD_PROBE_ROOT = "C:\cd-throwaway-agents"
$env:CD_PROBE_VERBOSE = "1"
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Set-Content "$results\sandbox.txt" "user $env:USERNAME, administrator $admin, Windows $([Environment]::OSVersion.Version)"

# Each run: the name its log and results take, the probe, and its arguments.
function Planned([string]$name, [string]$probe, [string]$arguments) {
    [pscustomobject]@{ Name = $name; Probe = $probe; Arguments = $arguments }
}
$runs = foreach ($probe in $Probes -split ",") {
    switch ($probe) {
        "claude-code-machine" {
            foreach ($version in Get-ChildItem C:\probe\claude -Directory) {
                Planned "$probe-$($version.Name)" $probe "--bin $($version.FullName)\claude.exe"
            }
        }
        "codex-machine" { Planned $probe $probe "--bin C:\probe\codex\$Codex" }
        "claude-desktop" { Planned $probe $probe "--installers C:\probe\desktop$(if ($SignedIn) { ' --signed-in' })" }
    }
}
function RunAll {
    foreach ($run in $runs) {
        $env:CD_PROBE_JSON = "$results\$($run.Name).json"
        # Through cmd, so that the log keeps Node's UTF-8, which Windows
        # PowerShell's redirection would rewrite as UTF-16.
        cmd /c "C:\probe\node\node.exe C:\probe\agents\$($run.Probe).mjs $($run.Arguments) > $results\$($run.Name).log 2>&1"
    }
    Set-Content "$results\done.txt" (Get-Date).ToString("o")
}
RunAll
if (-not (Test-Path C:\probe\agents\keep-open)) { shutdown /s /t 5; exit }
while ($true) {
    if (Test-Path "$results\rerun.txt") {
        Remove-Item "$results\rerun.txt", "$results\done.txt" -Force -ErrorAction SilentlyContinue
        RunAll
    }
    Start-Sleep -Seconds 5
}
