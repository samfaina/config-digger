# Builds the two installers step 12.2 updates between. run.ps1 calls it
# before the update run.
#
# The updater reads only releases/latest, so a release candidate is never
# offered an update from GitHub. Instead, installer A is the commit under
# test and installer B the same commit with the workspace version raised.
# Both point the updater at http://127.0.0.1:8765/latest.json, which the
# Sandbox serves, and trust a key made for this run only: the release key
# is never read. B is signed with that key, and its latest.json carries the
# signature, as the release's does.
#
# The builds run in a copy of the commit under -Work, a git archive, so
# they never lock the repository's own target folder and leave its .git as
# it was: no worktree, no ref. Each run copies the commit again, and keeps
# only the copy's target folder, cargo's build cache. The private key is
# deleted at the end, also when a build fails.
#
#   .\uat\auto\sandbox\build-update.ps1 -Commit v0.1.0-rc.3
#
# Writes to <Work>\installers: config-digger_<version>_x64-setup.exe for A,
# config-digger_<version B>_x64-setup.exe and .sig for B, and latest.json.
param(
    [Parameter(Mandatory)] [string]$Commit,
    [string]$Work = "$env:TEMP\cd-uat-sandbox\update",
    [string]$VersionB = "",
    [string]$Repo = (Resolve-Path "$PSScriptRoot\..\..\..").Path
)
$ErrorActionPreference = "Stop"

$tree = "$Work\tree"
$keys = "$Work\keys"
$out = "$Work\installers"
# A key a run that was stopped left behind goes too.
foreach ($folder in $keys, $out) { if (Test-Path $folder) { Remove-Item $folder -Recurse -Force } }
New-Item -ItemType Directory -Force $tree, $keys, $out | Out-Null
# With the \\?\ prefix, node_modules' long paths delete in Windows
# PowerShell 5.1 too.
Get-ChildItem $tree -Force | Where-Object { $_.Name -ne "target" } | ForEach-Object {
    if ($_.PSIsContainer) { [IO.Directory]::Delete("\\?\$($_.FullName)", $true) } else { Remove-Item -LiteralPath $_.FullName -Force }
}

git -C $Repo rev-parse --quiet --verify "$Commit^{commit}" | Out-Null
if ($LASTEXITCODE) { throw "no commit $Commit in ${Repo}: run git fetch --tags first" }
$archive = "$Work\tree.tar"
git -C $Repo archive --format=tar --output=$archive $Commit
if ($LASTEXITCODE) { throw "git archive $Commit failed" }
# Windows' own tar: Git's would read C: as a host name. -m dates the files
# now, not at the commit's time, so cargo never takes what the kept target
# folder built before from an older run as fresh.
& "$env:SystemRoot\System32\tar.exe" -xmf $archive -C $tree
if ($LASTEXITCODE) { throw "unpacking $archive failed" }
Remove-Item $archive
$ui = "$tree\crates\config-digger-app\ui"

$versionA = ((Get-Content "$tree\Cargo.toml") | Select-String '^version = "(.+)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $VersionB) { $VersionB = ($versionA -replace '-rc\.\d+$', '') + "-rc.99" }
Write-Host "A is $versionA, B is $VersionB"

Push-Location $ui
# The bundle takes the CLI from <checkout>\target\release, so the copy
# builds into its own target folder, whatever cargo's config says.
$saved = @{}
foreach ($name in "CARGO_TARGET_DIR", "GIT_CEILING_DIRECTORIES", "TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD") { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
$env:CARGO_TARGET_DIR = "$tree\target"
# The copy is no repository, so the app's build.rs records no commit for
# About, as from any source archive: never that of a repository -Work is in.
$env:GIT_CEILING_DIRECTORIES = $Work
$key = "$keys\uat-update.key"
try {
    npm ci --no-audit --no-fund
    if ($LASTEXITCODE) { throw "npm ci failed" }

    # A key for this run only. Its password only satisfies the signer.
    $password = "uat-throwaway"
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
    Raise "$tree\Cargo.lock" "(name = `"(config-digger-app|config-digger-cli|config-digger-core|config-digger-msix|xtask)`"\r?\nversion = )`"$a`"" "`$1`"$VersionB`"" -1
    Raise "$ui\package.json" "`"version`": `"$a`"" "`"version`": `"$VersionB`"" 1
    Raise "$ui\package-lock.json" "`"version`": `"$a`"" "`"version`": `"$VersionB`"" 2
    Build $VersionB

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
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    Remove-Item $key -ErrorAction SilentlyContinue
    Pop-Location
}
Get-ChildItem $out | ForEach-Object { Write-Host "built $($_.Name)" }
