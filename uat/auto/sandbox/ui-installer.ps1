# Dot-sourced by in-sandbox.ps1: drives the NSIS installer's and
# uninstaller's windows with UI Automation, in Windows PowerShell 5.1, as a
# tester's clicks do. Takes no screenshot.
#
# Without client-side providers, the managed UIA client sees every Win32
# control of an NSIS page as a Pane with no pattern. Controls are found by
# window class, name and AutomationId (Next, Install and Finish are 1,
# Cancel is 2), and driven with window messages on their handles. Clicks are
# posted, not sent: a sent BM_CLICK that ends a page waits for the next one.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$C = [System.Windows.Automation.Condition]
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class W32 {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, string l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, int m, IntPtr w, IntPtr l);
}
"@
$CLASS = @{ CheckBox = "Button"; RadioButton = "Button"; Button = "Button"; Edit = "Edit"; Text = "Static" }

# The top-level window whose name matches, waiting for it. A new
# installer's window is told apart by its process id, since the previous
# one may still be closing.
function Window([string]$pattern, [int]$seconds = 60, [int]$processId = 0) {
    $end = (Get-Date).AddSeconds($seconds)
    do {
        foreach ($w in $A::RootElement.FindAll($T::Children, $C::TrueCondition)) {
            if ($w.Current.Name -match $pattern -and ($processId -eq 0 -or $w.Current.ProcessId -eq $processId)) { return $w }
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $end)
    throw "no window matching '$pattern'"
}
# The first visible control of a kind whose name matches, waiting for it.
function Control($window, [string]$type, [string]$pattern, [int]$seconds = 15) {
    $end = (Get-Date).AddSeconds($seconds)
    do {
        foreach ($e in $window.FindAll($T::Descendants, $C::TrueCondition)) {
            if ($e.Current.ClassName -eq $CLASS[$type] -and $e.Current.Name -match $pattern -and -not $e.Current.IsOffscreen) { return $e }
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $end)
    $seen = @($window.FindAll($T::Descendants, $C::TrueCondition) | ForEach-Object { "$($_.Current.ClassName):$($_.Current.Name)" }) -join " | "
    throw "no $type matching '$pattern' in '$($window.Current.Name)', which holds: $seen"
}
function ById($window, [string]$automationId, [int]$seconds = 15) {
    $cond = New-Object System.Windows.Automation.PropertyCondition($A::AutomationIdProperty, $automationId)
    $end = (Get-Date).AddSeconds($seconds)
    do {
        $e = $window.FindFirst($T::Descendants, $cond)
        if ($e) { return $e }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $end)
    throw "no control with AutomationId $automationId in '$($window.Current.Name)'"
}
function Handle($e) { [IntPtr]$e.Current.NativeWindowHandle }
# BM_CLICK, BM_GETCHECK, WM_SETTEXT and WM_GETTEXT.
function Press($button) { [W32]::PostMessage((Handle $button), 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 700 }
function Ticked($box) { [int][W32]::SendMessage((Handle $box), 0x00F0, [IntPtr]::Zero, [IntPtr]::Zero) -eq 1 }
function SetText($edit, [string]$text) { [W32]::SendMessage((Handle $edit), 0x000C, [IntPtr]::Zero, $text) | Out-Null; Start-Sleep -Milliseconds 400 }
function Text($edit) { $sb = New-Object System.Text.StringBuilder 1024; [W32]::SendMessage((Handle $edit), 0x000D, [IntPtr]1024, $sb) | Out-Null; $sb.ToString() }
function Enabled($e) { [W32]::IsWindowEnabled((Handle $e)) }
function Next($window) { Press (ById $window "1") }
function WaitEnabled($e, [bool]$enabled, [int]$seconds = 5) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Enabled $e) -ne $enabled -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 200 }
    Enabled $e
}
# Every text a page shows, in one line.
function Texts($window) { @($window.FindAll($T::Descendants, $C::TrueCondition) | Where-Object { $_.Current.ClassName -eq "Static" -and $_.Current.Name } | ForEach-Object { $_.Current.Name }) -join " | " }

# Presses Next or Install until the Finish page, unticks its boxes, such as
# Run config-digger and Create desktop shortcut, and finishes.
function Finish($window, $process) {
    $end = (Get-Date).AddSeconds(180)
    while ((Get-Date) -lt $end) {
        $button = ById $window "1"
        if ($button.Current.Name -match "Finish") { break }
        if (Enabled $button) { Press $button; Start-Sleep -Seconds 1 } else { Start-Sleep -Milliseconds 500 }
    }
    foreach ($e in $window.FindAll($T::Descendants, $C::TrueCondition)) {
        if ($e.Current.ClassName -eq "Button" -and @("1", "2", "3") -notcontains $e.Current.AutomationId -and (Ticked $e)) { Press $e }
    }
    Press (ById $window "1")
    $process.WaitForExit(30000) | Out-Null
}
# The Already Installed page: Add/Reinstall components, then on.
function Reinstall($window) { Press (Control $window "RadioButton" "Add/Reinstall") }

# The uninstaller relaunches itself from a temporary copy, so its window is
# found by name.
function Uninstaller { Window "config-digger.*Uninstall" 60 }
# The warning showing now whose text matches, a top-level #32770 holding
# its text, or nothing.
function FindWarning([string]$pattern) {
    foreach ($top in $A::RootElement.FindAll($T::Children, $C::TrueCondition)) {
        if ($top.Current.ClassName -eq "#32770" -and (Texts $top) -match $pattern) { return $top }
    }
}
# The warning whose text matches, waiting for it: by default the Delete
# config-digger data one.
function Warning([string]$pattern = "Deleting config-digger data", [int]$seconds = 60) {
    $end = (Get-Date).AddSeconds($seconds)
    do {
        $warning = FindWarning $pattern
        if ($warning) { return $warning }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $end)
    throw "no warning matching '$pattern'"
}
# Presses Next or Install until the warning whose text matches shows, and
# returns it. Fails on the Finish page, which comes after any warning.
function UntilWarning($window, [string]$pattern, [int]$seconds = 180) {
    $end = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $end) {
        $warning = FindWarning $pattern
        if ($warning) { return $warning }
        $button = ById $window "1"
        if ($button.Current.Name -match "Finish") { throw "the Finish page came with no warning matching '$pattern'" }
        if (Enabled $button) { Press $button } else { Start-Sleep -Milliseconds 500 }
    }
    throw "no warning matching '$pattern'"
}
# The one button of a warning with only OK.
function OkButton($warning) { Control $warning "Button" "" }
# Waits for the uninstaller to remove the app, then closes its last page.
function EndUninstall($window, [string]$installDir) {
    $end = (Get-Date).AddSeconds(120)
    while ((Test-Path "$installDir\config-digger-app.exe") -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 500 }
    try { Press (Control $window "Button" "Finish|Close" 30) } catch { }
    Start-Sleep -Seconds 2
}
