# Dot-sourced by in-sandbox.ps1: what some checks compare, apart from the
# windows they read, so that sandbox.test.mjs runs them outside the Sandbox.
# Runs in Windows PowerShell 5.1.

function PlanStep($plan, [string]$id) { $plan.sections | ForEach-Object { $_.steps } | Where-Object { $_.id -eq $id } }

# The exact texts, between « and », of the line of step $id's expect that
# names $about, with <versión> as $version.
function PlanQuotes($plan, [string]$id, [string]$about, [string]$version) {
    $line = (PlanStep $plan $id).expect | Where-Object { $_.Contains($about) } | Select-Object -First 1
    if (-not $line) { throw "step $id expects nothing about '$about'" }
    # The marks as code points: Windows PowerShell 5.1 reads this file
    # without a byte order mark as ANSI.
    $open, $close, $placeholder = [char]0xAB, [char]0xBB, "<versi$([char]0xF3)n>"
    @([regex]::Matches($line, "$open([^$close]*)$close") | ForEach-Object { $_.Groups[1].Value.Replace($placeholder, $version) })
}

# Every file under $folder, with its SHA-256, in one line: the same line
# for the same files with the same bytes.
function FolderState([string]$folder) {
    if (-not (Test-Path -LiteralPath $folder)) { return "(no folder)" }
    $root = (Resolve-Path -LiteralPath $folder).Path.TrimEnd('\')
    # SHA-256 from .NET: Get-FileHash's module may not load, as 9.5's
    # Expand-Archive's doesn't.
    $sha = [Security.Cryptography.SHA256]::Create()
    @(Get-ChildItem -LiteralPath $root -Recurse -File -Force | Sort-Object FullName | ForEach-Object {
        $hash = -join ($sha.ComputeHash([IO.File]::ReadAllBytes($_.FullName)) | ForEach-Object { $_.ToString("X2") })
        "$($_.FullName.Substring($root.Length + 1)) $hash"
    }) -join ", "
}
