# TEMPORARY: what WebView2 does on the runner when the walk's session fails.
param([string]$App, [string]$Work)
$App = (Resolve-Path $App).Path
$exe = Split-Path $App -Leaf
whoami /groups | Select-String "Label"
$prof = Join-Path $Work "cd-throwaway-profile"
$env:USERPROFILE = $prof; $env:HOME = $prof
$env:APPDATA = "$prof\AppData\Roaming"; $env:LOCALAPPDATA = "$prof\AppData\Local"
$policy = "HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments"
foreach ($name in $exe, "*") {
    Write-Output "=== policy value name: $name"
    New-Item $policy -Force | Out-Null
    New-ItemProperty $policy -Name $name -Value "--remote-debugging-port=9222" -PropertyType String -Force | Out-Null
    reg query "HKLM\SOFTWARE\Policies\Microsoft\Edge\WebView2" /s
    $p = Start-Process $App -PassThru
    Start-Sleep 25
    Write-Output "app exited: $($p.HasExited)"
    Get-CimInstance Win32_Process | Where-Object { $_.Name -in "config-digger-app.exe", "msedgewebview2.exe" } |
        ForEach-Object { "$($_.ProcessId) <- $($_.ParentProcessId) $($_.Name): $("$($_.CommandLine)".Substring(0, [Math]::Min(700, "$($_.CommandLine)".Length)))" }
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object LocalPort -in 9222 | Format-Table -AutoSize | Out-String
    try { (Invoke-WebRequest http://127.0.0.1:9222/json/version -TimeoutSec 3).Content } catch { "9222: $($_.Exception.Message)" }
    Get-ChildItem "$prof\AppData\Local\io.github.samfaina.configdigger" -Recurse -Filter DevToolsActivePort -ErrorAction SilentlyContinue | ForEach-Object { "$($_.FullName): $(Get-Content $_.FullName -Raw)" }
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-ItemProperty $policy -Name $name -ErrorAction SilentlyContinue
    Start-Sleep 3
}
Get-ChildItem "$prof\AppData\Local\config-digger\logs" -ErrorAction SilentlyContinue | ForEach-Object { Get-Content $_.FullName -Tail 40 }
