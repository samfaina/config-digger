# Runs inside Windows Sandbox, from the LogonCommand of the .wsb run.ps1
# writes. Walks the UAT steps a clean Windows needs, checking what each
# step's expect says a tester sees, and appends one JSON line per check to
# C:\uat\results\results.jsonl:
#
#   {"time", "id": "<plan step id>", "check", "actual", "expected", "ok"}
#   {"time", "id": "<plan step id>", "blocked": "<why the step stopped>"}
#   {"time", "id": "<plan step id>", "note": "<what the step saw>"}
#
# -Run walk: 8.1, 9.5, 9.2, 9.3, 9.4, 13.1 (parked with the CLI), 13.2,
#            13.3 and 13.4, on the release's installer in C:\Descargas.
# -Run update: 12.2, from installer A to B in C:\uat\update, served on
#            127.0.0.1:8765 (see build-update.ps1).
#
# Runs in Windows PowerShell 5.1, which reads a script's accents only with a
# byte order mark: run.ps1 copies the scripts in with one. Takes no
# screenshot.
param([ValidateSet("walk", "update")] [string]$Run = "walk", [string]$Version)
# 9.5's Expand-Archive comes from a module that fails to load under "Stop"
# in the Sandbox, though it loads in a tester's window: load it first.
Import-Module Microsoft.PowerShell.Archive
$ErrorActionPreference = "Stop"
$results = "C:\uat\results"
$log = "$results\results.jsonl"
$installDir = "$env:LOCALAPPDATA\Programs\config-digger"
$dataLocal = "$env:LOCALAPPDATA\config-digger"
$dataRoaming = "$env:APPDATA\config-digger"
$choiceKey = "HKCU:\Software\samfaina\config-digger"
$plan = [IO.File]::ReadAllText("C:\uat\scripts\plan.json", [Text.Encoding]::UTF8) | ConvertFrom-Json

function Write-Line($fields) { Add-Content $log ([ordered]@{ time = (Get-Date).ToString("o") } + $fields | ConvertTo-Json -Compress) -Encoding UTF8 }
function Check([string]$id, [string]$what, $actual, $expected) {
    Write-Line ([ordered]@{ id = $id; check = $what; actual = "$actual"; expected = "$expected"; ok = ("$actual" -eq "$expected") })
}
function Note([string]$id, [string]$text) { Write-Line ([ordered]@{ id = $id; note = $text }) }
# Runs one step's checks. An error stops the step, which is then Blocked,
# and the walk goes on to the next step.
function Step([string]$id, [scriptblock]$body) {
    try { & $body } catch { Write-Line ([ordered]@{ id = $id; blocked = "$($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" }) }
}

# A command of a step, run exactly as the plan has it, with its output.
function PlanCommand([string]$id, [string]$label) {
    $step = $plan.sections | ForEach-Object { $_.steps } | Where-Object { $_.id -eq $id }
    $command = $step.commands | Where-Object { $_.label -eq $label }
    if (-not $command) { throw "step $id has no command '$label'" }
    $out = @(Invoke-Expression $command.text 2>&1 | ForEach-Object { "$_" })
    Note $id "$label -> $($out -join ' / ')"
    , $out
}
# A command as a new terminal runs it: with the PATH the registry holds now,
# which an open window does not see.
function InNewTerminal([scriptblock]$body) {
    $saved = $env:Path
    $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [Environment]::GetEnvironmentVariable("Path", "User")
    try { & $body } finally { $env:Path = $saved }
}
function CommandSource { InNewTerminal { $c = Get-Command config-digger -ErrorAction SilentlyContinue; if ($c) { $c.Source } else { "not recognized" } } }
function OnPath { @(([Environment]::GetEnvironmentVariable("Path", "User")) -split ';' | Where-Object { $_.TrimEnd('\') -ieq $installDir }).Count -gt 0 }
function PathChoice { (Get-ItemProperty $choiceKey -Name KeepPathAlone -ErrorAction SilentlyContinue).KeepPathAlone }
function Digger { & "$installDir\config-digger.exe" @args 2>&1 | ForEach-Object { "$_" } }
function OperationCount { @((Digger --json history | Out-String | ConvertFrom-Json).operations).Count }
function Installer { Get-ChildItem "C:\Descargas\config-digger_${Version}_x64-setup.exe" | Select-Object -First 1 -ExpandProperty FullName }

. C:\uat\scripts\ui-installer.ps1

try {
    Note "run" "sandbox user $env:USERNAME, culture $((Get-Culture).Name), PowerShell $($PSVersionTable.PSVersion), run $Run, version $Version"
    if ($Run -eq "update") {
        . C:\uat\scripts\update.ps1
        return
    }

    Step "8.1" {
        $out = PlanCommand "8.1" "Ver qué queda de config-digger"
        Check "8.1" "four lines" $out.Count 4
        Check "8.1" "every line False" (@($out | Where-Object { $_ -notmatch ': False$' }).Count) 0
    }

    Step "9.5" {
        $out = PlanCommand "9.5" "Buscar el runtime de VC++"
        Check "9.5" "VC++ runtime DLLs in System32" ($out -join ",") "False,False,False"
        $out = PlanCommand "9.5" "Probar el CLI del zip"
        Check "9.5" "the zip's CLI version" ($out | Select-Object -Last 1) "config-digger $Version"
        $out = PlanCommand "9.5" "Instalar sin preguntas y leer el PATH"
        Check "9.5" "the install folder among the PATH lines" ($out -contains "C:\Users\WDAGUtilityAccount\AppData\Local\Programs\config-digger") $true
        Check "9.5" "the last line" ($out | Select-Object -Last 1) "config-digger $Version"
        # 9.2 needs a PC that never had config-digger: uninstall silently and
        # delete what is left, as 8.1's second command does.
        Start-Process "$installDir\uninstall.exe" -ArgumentList "/S" -Wait
        $end = (Get-Date).AddSeconds(60)
        while ((Test-Path "$installDir\config-digger-app.exe") -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 500 }
        PlanCommand "8.1" "Borrar los datos de config-digger que queden" | Out-Null
        Check "9.5" "clean again for 9.2" ((PlanCommand "8.1" "Ver qué queda de config-digger") -join ",") "La app: False,Backups, History y Entries aparcados: False,Listas y ajustes: False,Elección de PATH: False"
    }

    Step "9.2" {
        $p = Start-Process (Installer) -PassThru
        $w = Window "config-digger.*Setup" 60 $p.Id
        $box = Control $w "CheckBox" "PATH"
        Check "9.2" "PATH box label" $box.Current.Name "Add the config-digger command to my PATH"
        Check "9.2" "PATH box starts ticked" (Ticked $box) $true
        Next $w
        $edit = Control $w "Edit" ""
        $proposed = Text $edit
        Check "9.2" "proposed folder" $proposed $installDir
        Note "9.2" "directory page: $(Texts $w)"
        Check "9.2" "directory page names the data folders" ((Texts $w).Contains("It can't be config-digger's data folder, %LOCALAPPDATA%\config-digger or %APPDATA%\config-digger, nor a folder inside or above one.")) $true
        SetText $edit $dataLocal
        Check "9.2" "Next greyed out on the data folder" (WaitEnabled (ById $w "1") $false) $false
        SetText $edit "$dataRoaming\sub"
        Check "9.2" "Next greyed out inside the roaming data folder" (WaitEnabled (ById $w "1") $false) $false
        SetText $edit $proposed
        Check "9.2" "Next enabled on the proposed folder again" (WaitEnabled (ById $w "1") $true) $true
        Finish $w $p
        Check "9.2" "Get-Command in a new terminal" (CommandSource) "$installDir\config-digger.exe"
        Check "9.2" "PATH choice saved as ticked" (PathChoice) 0
    }

    Step "9.3" {
        $p = Start-Process (Installer) -PassThru
        $w = Window "config-digger.*Setup" 60 $p.Id
        $box = Control $w "CheckBox" "PATH"
        Check "9.3" "PATH box starts ticked" (Ticked $box) $true
        Press $box
        Check "9.3" "PATH box unticked" (Ticked $box) $false
        Next $w
        Note "9.3" "already installed page: $(Texts $w)"
        Reinstall $w
        Finish $w $p
        Check "9.3" "Get-Command in a new terminal" (CommandSource) "not recognized"
        $out = PlanCommand "9.3" "Leer la elección de PATH guardada"
        Check "9.3" "KeepPathAlone" (PathChoice) 1
    }

    Step "9.4" {
        $p = Start-Process (Installer) -PassThru
        $w = Window "config-digger.*Setup" 60 $p.Id
        $box = Control $w "CheckBox" "PATH"
        Check "9.4" "PATH box starts unticked" (Ticked $box) $false
        Press $box
        Next $w
        Reinstall $w
        Finish $w $p
        Check "9.4" "Get-Command in a new terminal" (CommandSource) "$installDir\config-digger.exe"
    }

    # 13.1 parks cd-throwaway-desktop in the app. Here the CLI parks it, on
    # throwaway data under C:\cd-throwaway, so that 13.2 and 13.3 have a
    # parked Entry and a backup.
    Step "13.1" {
        & powershell -NoProfile -ExecutionPolicy Bypass -File C:\uat\scripts\cd-throwaway-setup.ps1 -Root C:\cd-throwaway | Out-Null
        Note "13.1" "homes add: $(Digger homes add --agent claude-desktop C:\cd-throwaway\claude-desktop-home)"
        $out = Digger --no-project disable cd-throwaway-desktop --yes
        Note "13.1" "disable: $($out -join ' / ')"
        Check "13.1" "the Disable is verified" (($out -join " ") -match "Disabled 1 Entry\. Operation \S+: verified\.") $true
        Check "13.1" "parked" (Test-Path "$dataLocal\parked\manifest.json") $true
    }

    $operations = 0
    Step "13.2" {
        $operations = OperationCount
        Note "13.2" "Operations before uninstalling: $operations"
        Start-Process "$installDir\uninstall.exe"
        $w = Uninstaller
        $box = Control $w "CheckBox" "Delete config-digger data"
        Check "13.2" "Delete config-digger data starts unticked" (Ticked $box) $false
        Next $w
        EndUninstall $w $installDir
        $out = PlanCommand "13.2" "Ver qué conserva la desinstalación"
        Check "13.2" "the app is gone" $out[0] "False"
        Check "13.2" "the parked Entry is kept" $out[1] "True"
        Check "13.2" "backups are kept" ([int]$out[2] -gt 0) $true
        Check "13.2" "Get-Command in a new terminal" (CommandSource) "not recognized"
        Start-Process (Installer) -ArgumentList "/S" -Wait
        Check "13.2" "History keeps every Operation after reinstalling" (OperationCount) $operations
        $out = Digger --no-project list cd-throwaway-desktop
        Note "13.2" "list cd-throwaway-desktop: $($out -join ' / ')"
        Check "13.2" "cd-throwaway-desktop still Disabled" (($out -join " ") -match "Disabled") $true
    }

    Step "13.3" {
        Start-Process "$installDir\uninstall.exe"
        $w = Uninstaller
        Press (Control $w "CheckBox" "Delete config-digger data")
        Next $w
        $mb = Warning
        $texts = Texts $mb
        Note "13.3" "warning: $texts"
        Check "13.3" "warning names the backups" ($texts.Contains("Deleting config-digger data deletes every backup, and with them the Undo history.")) $true
        Check "13.3" "warning names the parked Entries" ($texts.Contains("It also deletes these parked Entries, which the uninstaller never restores. To keep one, cancel and Enable it in config-digger first:")) $true
        Check "13.3" "one line per parking" ([regex]::Matches($texts, [regex]::Escape("cd-throwaway-desktop (Claude Desktop)")).Count) 1
        Check "13.3" "warning ends with OK or Cancel" ($texts.Contains("Click OK to delete the data, or Cancel to keep it and uninstall only the app.")) $true
        $cancel = ById $mb "2"
        Check "13.3" "Cancel is the default button" ($cancel.Current.HasKeyboardFocus) $true
        Press $cancel
        EndUninstall $w $installDir
        $out = PlanCommand "13.3" "Ver qué conserva la desinstalación"
        Check "13.3" "the app is gone" $out[0] "False"
        Check "13.3" "the parked Entry is kept" $out[1] "True"
        Check "13.3" "backups are kept" ([int]$out[2] -gt 0) $true
    }

    Step "13.4" {
        Start-Process (Installer) -ArgumentList "/S" -Wait
        Check "13.4" "reinstalled" (Test-Path "$installDir\config-digger-app.exe") $true
        Start-Process "$installDir\uninstall.exe"
        $w = Uninstaller
        Press (Control $w "CheckBox" "Delete config-digger data")
        Next $w
        Press (ById (Warning) "1")
        EndUninstall $w $installDir
        $out = PlanCommand "13.4" "Ver qué queda de config-digger"
        Check "13.4" "four lines" $out.Count 4
        Check "13.4" "every line False" (@($out | Where-Object { $_ -notmatch ': False$' }).Count) 0
        Check "13.4" "off the PATH" (OnPath) $false
    }
} catch {
    Write-Line ([ordered]@{ id = "run"; blocked = "$($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" })
} finally {
    Note "run" "done"
    Set-Content "$results\done.txt" (Get-Date).ToString("o")
    if (-not (Test-Path C:\uat\scripts\keep-open)) { shutdown /s /t 5 }
}
