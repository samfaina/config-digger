# Answers the Windows folder picker that config-digger opens on Choose…:
# types -Folder in its folder box and presses its select button, through UI
# Automation, as the tester does with the mouse. Finds the picker among the
# top-level windows of the config-digger-app process.
param(
    [Parameter(Mandatory)][string]$Folder,
    [int]$TimeoutSeconds = 15
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
# The managed UIA client shows the picker's Win32 controls without patterns,
# so the script types and clicks with window messages on their handles, as
# uat/auto/sandbox does on the installer's pages. The click is posted, not
# sent: a sent click waits for the picker to close.
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class PickerWin32 {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr hWnd, int msg, IntPtr wParam, string lParam);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, int msg, IntPtr wParam, IntPtr lParam);
}
"@

$root = [System.Windows.Automation.AutomationElement]::RootElement
$apps = @(Get-Process config-digger-app -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
if (-not $apps) { throw "config-digger-app is not running" }

$dialog = $null
$end = (Get-Date).AddSeconds($TimeoutSeconds)
while (-not $dialog -and (Get-Date) -lt $end) {
    $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($w in $windows) {
        if ($apps -contains $w.Current.ProcessId -and $w.Current.ClassName -eq "#32770") { $dialog = $w }
    }
    if (-not $dialog) {
        # The picker may be owned by the app's window rather than a top-level one.
        foreach ($w in $windows) {
            if ($apps -contains $w.Current.ProcessId) {
                $inner = $w.FindFirst([System.Windows.Automation.TreeScope]::Children,
                    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "#32770")))
                if ($inner) { $dialog = $inner }
            }
        }
    }
    if (-not $dialog) { Start-Sleep -Milliseconds 250 }
}
if (-not $dialog) { throw "the folder picker did not open" }
Write-Output "picker: $($dialog.Current.Name)"

# The folder box is the dialog's edit field (AutomationId 1152), and its
# select button has AutomationId 1, whatever the language says on it.
$editCond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "1152")
$edit = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $editCond)
if (-not $edit) { throw "the folder picker has no folder box" }
$WM_SETTEXT = 0x000C
$BM_CLICK = 0x00F5
[void][PickerWin32]::SendMessage([IntPtr]$edit.Current.NativeWindowHandle, $WM_SETTEXT, [IntPtr]::Zero, $Folder)

$okCond = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "1")),
    (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "Button")))
$ok = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $okCond)
if (-not $ok) { throw "the folder picker has no select button" }
Write-Output "button: $($ok.Current.Name)"
[void][PickerWin32]::PostMessage([IntPtr]$ok.Current.NativeWindowHandle, $BM_CLICK, [IntPtr]::Zero, [IntPtr]::Zero)
