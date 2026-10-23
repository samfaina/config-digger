# Dot-sourced by in-sandbox.ps1 -Run update: step 12.2, an update from the
# app. Installs A from C:\uat\update, gives it History on throwaway data,
# serves B's latest.json on 127.0.0.1:8765 (serve.ps1), and presses About,
# Check now and Install and restart in the app. Then checks what 12.2's
# expect asks for once the app restarts on B.
#
# WebDriver sessions don't open inside the Sandbox (DevToolsActivePort file
# doesn't exist), so the app's buttons are pressed through UI Automation,
# which reads WebView2's accessibility tree.
$versionB = ([IO.File]::ReadAllText("C:\uat\update\latest.json") | ConvertFrom-Json).version
$installerA = Get-ChildItem C:\uat\update\config-digger_*_x64-setup.exe | Where-Object { $_.Name -ne "config-digger_${versionB}_x64-setup.exe" } | Select-Object -First 1
$versionA = $installerA.Name -replace '^config-digger_(.+)_x64-setup\.exe$', '$1'
Note "12.2" "A is $versionA, B is $versionB"

function AppWindow([int]$seconds = 90) {
    $end = (Get-Date).AddSeconds($seconds)
    do {
        $app = Get-Process config-digger-app -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($app) {
            foreach ($top in $A::RootElement.FindAll($T::Children, $C::TrueCondition)) {
                if ($top.Current.ProcessId -eq $app.Id -and $top.Current.Name -eq "config-digger") { return $top }
            }
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $end)
    throw "no config-digger window"
}
function AppElement($window, [string]$type, [string]$pattern, [int]$seconds = 30) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::$type)
    $end = (Get-Date).AddSeconds($seconds)
    do {
        foreach ($e in $window.FindAll($T::Descendants, $cond)) { if ($e.Current.Name -match $pattern) { return $e } }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $end)
    throw "no $type matching '$pattern'"
}
# Every text the page shows, joined: a sentence may span several nodes.
function AppText($window) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)
    (@($window.FindAll($T::Descendants, $cond) | ForEach-Object { $_.Current.Name }) -join "") -replace '\s+', ' '
}
function WaitAppText($window, [string]$pattern, [int]$seconds = 60) {
    $end = (Get-Date).AddSeconds($seconds)
    do {
        $text = AppText $window
        if ($text -match $pattern) { return $Matches[0] }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $end)
    Note "12.2" "no text matching '$pattern' in: $text"
    "(not shown)"
}
function Click($e) { $e.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke() }

Step "12.2" {
    Start-Process $installerA.FullName -ArgumentList "/S" -Wait
    Check "12.2" "A installed" ((Digger --version) -join "") "config-digger $versionA"
    # History before the update: one Disable on throwaway data.
    & powershell -NoProfile -ExecutionPolicy Bypass -File C:\uat\scripts\cd-throwaway-setup.ps1 -Root C:\cd-throwaway | Out-Null
    Digger homes add --agent claude-desktop C:\cd-throwaway\claude-desktop-home | Out-Null
    Note "12.2" "disable: $((Digger --no-project disable cd-throwaway-desktop --yes) -join ' / ')"
    $operations = OperationCount
    Note "12.2" "Operations before the update: $operations"

    Start-Process powershell.exe -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "C:\uat\scripts\serve.ps1" -WindowStyle Hidden | Out-Null
    Start-Sleep -Seconds 2
    Start-Process "$installDir\config-digger-app.exe" | Out-Null
    $w = AppWindow
    Click (AppElement $w "Button" "^About$")
    Check "12.2" "About shows A's version" (WaitAppText $w "config-digger $([regex]::Escape($versionA))") "config-digger $versionA"
    Note "12.2" "About before the check: $(WaitAppText $w 'Last checked: [^.]*\.')"
    Click (AppElement $w "Button" "^Check now$")
    Check "12.2" "the update is offered" (WaitAppText $w "config-digger \S+ is available\.") "config-digger $versionB is available."
    Click (AppElement $w "Button" "^Install and restart$")

    $end = (Get-Date).AddSeconds(240)
    do { Start-Sleep -Seconds 2; $now = (Digger --version) -join "" } while ($now -ne "config-digger $versionB" -and (Get-Date) -lt $end)
    Check "12.2" "the CLI is replaced" $now "config-digger $versionB"
    Start-Sleep -Seconds 5
    $w = AppWindow 120
    $app = Get-Process config-digger-app | Select-Object -First 1
    Check "12.2" "the app restarts on B" $app.MainModule.FileVersionInfo.ProductVersion $versionB
    Click (AppElement $w "Button" "^About$")
    Check "12.2" "About shows B's version after the restart" (WaitAppText $w "config-digger $([regex]::Escape($versionB))") "config-digger $versionB"
    $out = InNewTerminal { config-digger --version; (Get-Command config-digger).Source }
    Check "12.2" "config-digger --version in a new terminal" $out[0] "config-digger $versionB"
    Check "12.2" "Get-Command in a new terminal" $out[1] "$installDir\config-digger.exe"
    Check "12.2" "History keeps its Operations" (OperationCount) $operations
    Get-Content "$results\serve.log" -ErrorAction SilentlyContinue | ForEach-Object { Note "12.2" "served: $_" }
    Get-ChildItem "$dataLocal\logs\*.log" -ErrorAction SilentlyContinue | ForEach-Object {
        Get-Content $_.FullName | Select-String "update|app started" | ForEach-Object { Note "12.2" "app log: $($_.Line)" }
    }
}
