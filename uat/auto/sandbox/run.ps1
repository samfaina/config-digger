# Walks the UAT steps that need a clean Windows in Windows Sandbox,
# unattended, and writes their verdicts in the shape of the runner's
# results: { id, status, notes, time, version, by, evidence }.
#
#   .\uat\auto\sandbox\run.ps1 -Version 0.1.0-rc.3
#   .\uat\auto\sandbox\run.ps1 -Version 0.1.0-rc.3 -Runs walk
#
# Runs:
# - walk: 8.1, 9.5, 9.2, 9.3, 9.4, 13.2, 13.3 and 13.4 on the release's
#   installer and CLI zip, downloaded with gh into the folder the Sandbox
#   sees as C:\Descargas, as 9.5's commands expect.
# - update: 12.2, between the two installers build-update.ps1 writes to
#   -Update. Run build-update.ps1 first.
#
# Each run opens one Sandbox, whose LogonCommand runs in-sandbox.ps1, waits
# for its results\done.txt, and waits for the Sandbox to close, since only
# one runs at a time. Needs Windows Sandbox (Pro, Enterprise or Education)
# and gh. Everything it installs stays in the Sandbox. It takes no
# screenshot, and writes nothing in the repository.
param(
    [Parameter(Mandatory)] [string]$Version,
    [string]$Work = "$env:TEMP\cd-uat-sandbox",
    [ValidateSet("walk", "update")] [string[]]$Runs = @("walk", "update"),
    [string]$Update = "$env:TEMP\cd-uat-update\installers",
    [string]$Out = "",
    [int]$TimeoutMinutes = 40,
    # A folder holding the installer and the CLI zip of a build that is not
    # released, to walk it instead of downloading the release's.
    [string]$From = ""
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..\..\..").Path
if (-not $Out) { $Out = "$Work\uat-results.json" }
$release = "$Work\release"
$scripts = "$Work\scripts"
New-Item -ItemType Directory -Force $Work, $release, $scripts | Out-Null

# The release's assets, named after its version.
$setupName = "config-digger_${Version}_x64-setup.exe"
$zipName = "config-digger_${Version}_x64-cli.zip"
Remove-Item "$release\*" -Force -ErrorAction SilentlyContinue
if ($From) {
    Copy-Item "$From\$setupName", "$From\$zipName" $release
} else {
    gh release download "v$Version" --repo samfaina/config-digger --pattern $setupName --pattern $zipName --dir $release --clobber
    if ($LASTEXITCODE) { throw "gh release download v$Version failed" }
}

# The scripts, the plan and the throwaway data script, each .ps1 with a byte
# order mark so that Windows PowerShell 5.1 reads its accents.
foreach ($file in @(Get-ChildItem "$PSScriptRoot\*.ps1") + (Get-Item "$repo\uat\data\cd-throwaway-setup.ps1")) {
    [IO.File]::WriteAllText("$scripts\$($file.Name)", [IO.File]::ReadAllText($file.FullName), [Text.UTF8Encoding]::new($true))
}
Copy-Item "$repo\uat\plan.json" $scripts -Force

function WaitSandboxClosed([int]$seconds = 180) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Get-Process -Name WindowsSandbox* -ErrorAction SilentlyContinue) -and (Get-Date) -lt $end) { Start-Sleep -Seconds 3 }
}
function Mapped([string]$hostFolder, [string]$sandbox, [bool]$readOnly) {
    "<MappedFolder><HostFolder>$hostFolder</HostFolder><SandboxFolder>$sandbox</SandboxFolder><ReadOnly>$($readOnly.ToString().ToLower())</ReadOnly></MappedFolder>"
}

$logs = @()
foreach ($run in $Runs) {
    $results = "$Work\results-$run"
    if (Test-Path $results) { Remove-Item $results -Recurse -Force }
    New-Item -ItemType Directory $results | Out-Null
    $folders = @(
        (Mapped $release "C:\Descargas" $true),
        (Mapped $scripts "C:\uat\scripts" $true),
        (Mapped $results "C:\uat\results" $false)
    )
    if ($run -eq "update") {
        if (-not (Test-Path "$Update\latest.json")) { throw "no $Update\latest.json: run build-update.ps1 first" }
        $folders += Mapped (Resolve-Path $Update).Path "C:\uat\update" $true
    }
    $wsb = "$Work\$run.wsb"
    Set-Content $wsb @"
<Configuration>
  <Networking>Enable</Networking>
  <vGPU>Disable</vGPU>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <MemoryInMB>4096</MemoryInMB>
  <MappedFolders>$($folders -join '')</MappedFolders>
  <LogonCommand><Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\uat\scripts\in-sandbox.ps1 -Run $run -Version $Version</Command></LogonCommand>
</Configuration>
"@
    WaitSandboxClosed
    Write-Host "$run`: opening Windows Sandbox"
    Start-Process $wsb
    $end = (Get-Date).AddMinutes($TimeoutMinutes)
    while (-not (Test-Path "$results\done.txt") -and (Get-Date) -lt $end) { Start-Sleep -Seconds 10 }
    if (-not (Test-Path "$results\done.txt")) { Write-Warning "$run`: no done.txt after $TimeoutMinutes minutes" }
    WaitSandboxClosed
    $logs += "$results\results.jsonl"
}

# --- Verdicts, in the runner's shape. The notes are in the tester's
# language, as the plan is.
$about = @{
    "8.1"  = "En un Windows Sandbox nuevo, la primera orden del paso, tal cual. Sin comprobar: desinstalar desde Configuración de Windows, que un PC limpio no necesita."
    "9.5"  = "Las tres órdenes del paso, tal cual, en Windows Sandbox con la release en C:\Descargas."
    "9.2"  = "Instalador de la release manejado con UI Automation en Windows Sandbox; «Get-Command» con el PATH del registro, como una ventana nueva. Sin comprobar: el aviso «Windows protegió su PC» de SmartScreen, que no sale en el Sandbox porque un archivo de una carpeta mapeada no lleva la marca de la web."
    "9.3"  = "Instalador de la release manejado con UI Automation en Windows Sandbox."
    "9.4"  = "Instalador de la release manejado con UI Automation en Windows Sandbox."
    "13.2" = "Desinstalador (uninstall.exe, no Configuración de Windows) manejado con UI Automation en Windows Sandbox. cd-throwaway-desktop se aparcó con el CLI (homes add y disable) en C:\cd-throwaway, no E:\cd-throwaway. History y el State, con el CLI (history --json y list), no en la app. Reinstalado con /S."
    "13.3" = "Desinstalador manejado con UI Automation en Windows Sandbox, con una Entry aparcada una vez: el paso 7.2 no se hace en el Sandbox, así que se espera una línea."
    "13.4" = "Reinstalado con /S y desinstalado con UI Automation en Windows Sandbox; la orden del paso, tal cual."
    "12.2" = "El paso pide Bloqueado con una release candidate, porque el updater solo lee releases/latest. Aquí se comprobó la actualización de verdad contra un endpoint local: A y B compilados del mismo commit con build-update.ps1, con una clave de usar y tirar hecha para esta ejecución (no la de la release, que sigue en su secreto de GitHub) y http://127.0.0.1:8765/latest.json, en Windows Sandbox. About, Check now e Install and restart, pulsados con UI Automation; History, con el CLI."
}
$lines = foreach ($log in $logs) { if (Test-Path $log) { Get-Content $log -Encoding UTF8 | ForEach-Object { $_ | ConvertFrom-Json } } }
$verdicts = [System.Collections.Generic.List[object]]::new()

# 9.1 is checked here: the release has the installer, named after its version.
if (-not $From) { $verdicts.Add([ordered]@{
    id = "9.1"
    status = if (Test-Path "$release\$setupName") { "ok" } else { "ko" }
    notes = "gh release download v$Version baja «$setupName». Sin comprobar: bajarlo desde el navegador a Descargas."
    time = (Get-Date).ToUniversalTime().ToString("o")
    version = $Version
    by = "local"
    evidence = "uat/auto/sandbox/run.ps1: gh release download"
}) }

$order = @("8.1", "9.5", "9.2", "9.3", "9.4", "12.2", "13.2", "13.3", "13.4")
foreach ($id in $order) {
    $mine = @($lines | Where-Object { $_.id -eq $id })
    if (-not $mine) { continue }
    $checks = @($mine | Where-Object { $null -ne $_.check })
    $failed = @($checks | Where-Object { -not $_.ok })
    $blocked = @($mine | Where-Object { $_.blocked })
    $status = if ($failed) { "ko" } elseif ($blocked -or -not $checks) { "blocked" } else { "ok" }
    $notes = @($about[$id])
    $notes += "$(@($checks | Where-Object { $_.ok }).Count) de $($checks.Count) comprobaciones OK."
    foreach ($f in $failed) { $notes += "KO en «$($f.check)»: sale «$($f.actual)», se esperaba «$($f.expected)»." }
    foreach ($b in $blocked) { $notes += "El script se paró: $(($b.blocked -split "`n")[0])" }
    $verdicts.Add([ordered]@{
        id = $id
        status = $status
        notes = $notes -join " "
        time = ($mine | Select-Object -Last 1).time
        version = $Version
        by = "local"
        evidence = "uat/auto/sandbox/in-sandbox.ps1 step $id; $(($logs | Where-Object { Test-Path $_ }) -join ', ')"
    })
}
# A step with no line at all never ran: the run stopped before it.
$ran = $verdicts | ForEach-Object { $_.id }
$expected = @()
if ($Runs -contains "walk") { $expected += "8.1", "9.5", "9.2", "9.3", "9.4", "13.2", "13.3", "13.4" }
if ($Runs -contains "update") { $expected += "12.2" }
foreach ($id in $expected | Where-Object { $ran -notcontains $_ }) {
    $stop = @($lines | Where-Object { $_.id -eq "run" -and $_.blocked }) | Select-Object -First 1
    $verdicts.Add([ordered]@{
        id = $id; status = "blocked"; version = $Version; by = "local"
        notes = "El script de Windows Sandbox no llegó a este paso. $(if ($stop) { ($stop.blocked -split "`n")[0] })"
        time = (Get-Date).ToUniversalTime().ToString("o")
        evidence = ($logs -join ", ")
    })
}

ConvertTo-Json @($verdicts) -Depth 4 | Set-Content $Out -Encoding utf8
$verdicts | ForEach-Object { Write-Host ("{0,-5} {1}" -f $_.id, $_.status) }
Write-Host "wrote $Out"
