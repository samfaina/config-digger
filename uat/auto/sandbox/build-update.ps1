# Builds the two installers step 12.2 updates between, for run.ps1 -Update.
#
# The updater reads only releases/latest, so a release candidate is never
# offered an update from GitHub. Instead, installer A is the commit under
# test and installer B the same commit with the workspace version raised.
# Both point the updater at http://127.0.0.1:8765/latest.json, which the
# Sandbox serves, and trust a key made for this run only: the release key
# never leaves its GitHub secret. B is signed with that key, and its
# latest.json carries the signature, as the release's does.
#
# The builds run in a git worktree under -Work, so they never lock the
# repository's own target folder. The private key is deleted at the end.
#
#   .\uat\auto\sandbox\build-update.ps1 -Commit v0.1.0-rc.3 -Work $env:TEMP\cd-uat-update
#
# Writes to <Work>\installers: config-digger_<version>_x64-setup.exe for A,
# config-digger_<version B>_x64-setup.exe and .sig for B, and latest.json.
param(
    [Parameter(Mandatory)] [string]$Commit,
    [string]$Work = "$env:TEMP\cd-uat-update",
    [string]$VersionB = "",
    [string]$Repo = (Resolve-Path "$PSScriptRoot\..\..\..").Path
)
$ErrorActionPreference = "Stop"

$tree = "$Work\tree"
$keys = "$Work\keys"
$out = "$Work\installers"
New-Item -ItemType Directory -Force $keys, $out | Out-Null

if (-not (Test-Path $tree)) { git -C $Repo worktree add --detach $tree $Commit; if ($LASTEXITCODE) { throw "git worktree add failed" } }
else { git -C $tree checkout --detach --force $Commit; if ($LASTEXITCODE) { throw "git checkout failed" } }
$ui = "$tree\crates\config-digger-app\ui"

$versionA = ((Get-Content "$tree\Cargo.toml") | Select-String '^version = "(.+)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $VersionB) { $VersionB = ($versionA -replace '-rc\.\d+$', '') + "-rc.99" }
Write-Host "A is $versionA, B is $VersionB"

Push-Location $ui
# The bundle takes the CLI from <checkout>\target\release, so the worktree
# builds into its own target folder, whatever cargo's config says.
$savedTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = "$tree\target"
try {
    npm ci --no-audit --no-fund
    if ($LASTEXITCODE) { throw "npm ci failed" }

    # A key for this run only. Its password only satisfies the signer.
    $password = "uat-throwaway"
    $key = "$keys\uat-update.key"
    Remove-Item "$key", "$key.pub" -ErrorAction SilentlyContinue
    & .\node_modules\.bin\tauri.cmd signer generate -w $key -p $password --ci | Out-Null
    if ($LASTEXITCODE) { throw "tauri signer generate failed" }
    $override = "$keys\updater.conf.json"
    [ordered]@{ plugins = [ordered]@{ updater = [ordered]@{
        pubkey = (Get-Content "$key.pub" -Raw).Trim()
        endpoints = @("http://127.0.0.1:8765/latest.json")
        dangerousInsecureTransportProtocol = $true
    } } } | ConvertTo-Json -Depth 5 | Set-Content $override

    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $key -Raw).Trim()
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $password

    function Build([string]$version) {
        # As release.yml builds it: npm's tauri script runs from the app's crate.
        npm run tauri -- build --config tauri.bundle.conf.json --config $override
        if ($LASTEXITCODE) { throw "tauri build of $version failed" }
        $setup = "$tree\target\release\bundle\nsis\config-digger_${version}_x64-setup.exe"
        Copy-Item $setup, "$setup.sig" $out -Force
    }

    Build $versionA

    # B: the same commit with the workspace version raised, in the four
    # files that carry it, so both executables report it.
    function Raise([string]$file, [string]$pattern, [string]$replacement, [int]$count) {
        $text = [IO.File]::ReadAllText($file)
        $regex = [regex]::new($pattern, [Text.RegularExpressions.RegexOptions]::Multiline)
        $raised = $regex.Replace($text, $replacement, $count)
        if ($raised -eq $text) { throw "no version to raise in $file" }
        [IO.File]::WriteAllText($file, $raised)
    }
    $a = [regex]::Escape($versionA)
    Raise "$tree\Cargo.toml" "^version = `"$a`"" "version = `"$VersionB`"" 1
    Raise "$tree\Cargo.lock" "(name = `"(config-digger-app|config-digger-cli|config-digger-core|xtask)`"\r?\nversion = )`"$a`"" "`$1`"$VersionB`"" -1
    Raise "$ui\package.json" "`"version`": `"$a`"" "`"version`": `"$VersionB`"" 1
    Raise "$ui\package-lock.json" "`"version`": `"$a`"" "`"version`": `"$VersionB`"" 2
    try { Build $VersionB } finally { git -C $tree checkout -- . }

    $name = "config-digger_${VersionB}_x64-setup.exe"
    $signature = (Get-Content "$out\$name.sig" -Raw).Trim()
    $platform = [ordered]@{ url = "http://127.0.0.1:8765/$name"; signature = $signature }
    [ordered]@{
        version = $VersionB
        notes = "UAT update test"
        pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        platforms = [ordered]@{ "windows-x86_64" = $platform; "windows-x86_64-nsis" = $platform }
    } | ConvertTo-Json -Depth 5 | Set-Content "$out\latest.json"
} finally {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item "$keys\uat-update.key" -ErrorAction SilentlyContinue
    $env:CARGO_TARGET_DIR = $savedTarget
    Pop-Location
}
Get-ChildItem $out | ForEach-Object { Write-Host "built $($_.Name)" }
