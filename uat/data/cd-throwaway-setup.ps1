# Creates the throwaway Agent homes and Projects for the manual UAT walk of
# config-digger (uat/plan.json, see docs/agents/uat.md). Every Entry is named
# cd-throwaway-* and every Secret is fake (not-a-real-secret). Nothing outside
# -Root is written, and -Root must be a folder called cd-throwaway.
#
#   .\uat\data\cd-throwaway-setup.ps1          creates E:\cd-throwaway
#   .\uat\data\cd-throwaway-setup.ps1 -Reset   deletes E:\cd-throwaway and creates it again
#   .\uat\data\cd-throwaway-setup.ps1 -Root D:\tmp\cd-throwaway -Reset
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -Symlink   adds a skill behind a relative folder symlink
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -VsCode    adds VS Code profiles in vscode-appdata, and a workspace file with an error
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -Cursor    adds Cursor in cursor-appdata and a git Project with a nested skill
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -NameRule  adds Claude Code skills whose folder names break its name rule
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -PlainString  adds a Project whose Claude Code settings hold plain-string MCP lists
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -CodexLayer   adds a git Project whose git-versioned Codex layer declares a server
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -Frontmatter  adds Codex skills whose frontmatter is not YAML
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -DesktopOverlay  adds a Claude Desktop package beside a classic folder in desktop-appdata
param(
    [string]$Root = "E:\cd-throwaway",
    [switch]$Reset,
    [switch]$Symlink,
    [switch]$VsCode,
    [switch]$Cursor,
    [switch]$NameRule,
    [switch]$PlainString,
    [switch]$CodexLayer,
    [switch]$Frontmatter,
    [switch]$DesktopOverlay
)
$ErrorActionPreference = "Stop"

if ((Split-Path $Root -Leaf) -ne "cd-throwaway") {
    throw "The folder must be called cd-throwaway, so that -Reset can never delete anything else: $Root"
}
if (Test-Path $Root) {
    if (-not $Reset) {
        throw "$Root already exists. Run the script again with -Reset to delete it and start over."
    }
    Get-ChildItem $Root -Recurse -File | ForEach-Object { $_.IsReadOnly = $false }
    Remove-Item $Root -Recurse -Force
}

# Strict UTF-8 without BOM, in Windows PowerShell 5.1 and PowerShell 7 alike.
function Write-TestFile([string]$Path, [string]$Text) {
    New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

# Claude Code records Projects with forward slashes.
$slash = $Root -replace '\\', '/'

# A folder whose path, under even the shortest root, is too long for one line
# of Projects & homes in the smallest window, so that step 10.10 sees where a
# Project's path and a home's name and folder wrap.
$long = "paths-that-wrap\in-the-smallest-window\of-config-digger"
$longSlash = $long -replace '\\', '/'

# Claude Code home: a user server, a Project the user trusts, so that its
# own settings approve its server, with a local server, a missing Project
# with one leftover, a missing Project with nothing but a trust, as
# Claude Code keeps for a deleted worktree, a Project to hide, and a Project
# whose path wraps.
Write-TestFile "$Root\claude-code-home\.claude.json" @"
{
  "mcpServers": {
    "cd-throwaway-user": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_not-a-real-secret" }
    }
  },
  "projects": {
    "$slash/project": {
      "hasTrustDialogAccepted": true,
      "mcpServers": {
        "cd-throwaway-local": {
          "type": "http",
          "url": "https://example.invalid/mcp",
          "headers": { "Authorization": "Bearer not-a-real-secret" }
        }
      }
    },
    "$slash/project-gone": {
      "mcpServers": {
        "cd-throwaway-leftover": { "command": "npx", "args": ["-y", "cd-throwaway-leftover"] }
      }
    },
    "$slash/project-deleted": { "hasTrustDialogAccepted": true },
    "$slash/project-hide": {},
    "$slash/$longSlash/project-long": {}
  }
}
"@
Write-TestFile "$Root\claude-code-home\.claude\skills\cd-throwaway-user-skill\SKILL.md" @"
---
name: cd-throwaway-user-skill
description: Throwaway skill for testing config-digger. Does nothing.
---
"@

# The Project Claude Code knows: a project server, approved, and a skill.
Write-TestFile "$Root\project\.mcp.json" @"
{
  "mcpServers": {
    "cd-throwaway-project": {
      "command": "npx",
      "args": ["-y", "cd-throwaway-project", "--token", "not-a-real-secret"]
    }
  }
}
"@
Write-TestFile "$Root\project\.claude\settings.json" @"
{
  "enableAllProjectMcpServers": true
}
"@
Write-TestFile "$Root\project\.claude\skills\cd-throwaway-project-skill\SKILL.md" @"
---
name: cd-throwaway-project-skill
description: Throwaway skill for testing config-digger. Does nothing.
---
"@

# A Project to hide, the one whose path wraps, and one that no registry knows,
# to add by hand.
New-Item -ItemType Directory -Force "$Root\project-hide" | Out-Null
New-Item -ItemType Directory -Force "$Root\$long\project-long" | Out-Null
Write-TestFile "$Root\project-extra\.mcp.json" @"
{
  "mcpServers": {
    "cd-throwaway-extra": { "command": "npx", "args": ["-y", "cd-throwaway-extra"] }
  }
}
"@

# Codex home: Disable writes enabled = false here, which the Undo tests use.
Write-TestFile "$Root\codex-home\config.toml" @"
[mcp_servers.cd-throwaway-codex]
command = "npx"
args = ["-y", "cd-throwaway-codex", "--api-key", "not-a-real-secret"]
"@

# Claude Desktop home: Disable parks its servers, which the uninstall test
# uses. The UAT adds it before step 10.10, as the fixtures do, so its folder,
# which is also its name in Projects & homes, wraps.
Write-TestFile "$Root\$long\claude-desktop-home\claude_desktop_config.json" @"
{
  "mcpServers": {
    "cd-throwaway-desktop": { "command": "npx", "args": ["-y", "cd-throwaway-desktop"] }
  }
}
"@

# A second Claude Desktop home: its server, once parked, is Stranded when the
# home is forgotten.
Write-TestFile "$Root\claude-desktop-stranded-home\claude_desktop_config.json" @"
{
  "mcpServers": {
    "cd-throwaway-stranded": { "command": "npx", "args": ["-y", "cd-throwaway-stranded"], "env": { "API_TOKEN": "not-a-real-secret" } }
  }
}
"@

# A Claude Code home whose Project servers are a list, which fails only that
# location: the user server is still read.
Write-TestFile "$Root\claude-code-shape-home\.claude.json" @"
{
  "mcpServers": {
    "cd-throwaway-shape-user": { "command": "npx", "args": ["-y", "cd-throwaway-shape-user"] }
  },
  "projects": {
    "$slash/project": { "hasTrustDialogAccepted": true, "mcpServers": [] }
  }
}
"@

# A user folder whose default Claude Code home holds a second leftover of the
# missing Project. Step 18.1 starts the app with USERPROFILE set to it and
# hides that home, which Remove leftovers then leaves out: only a home the
# app discovers can be hidden.
Write-TestFile "$Root\claude-code-profile\.claude.json" @"
{
  "projects": {
    "$slash/project-gone": {
      "mcpServers": {
        "cd-throwaway-second-leftover": { "command": "npx", "args": ["-y", "cd-throwaway-second-leftover"] }
      }
    }
  }
}
"@

# A Claude Code home signed in to a claude.ai account, with a plugin synced
# from it, which Claude Code keeps in the account's folder and names by its
# manifest, and the plugin sync gate cached on, on 2 Oct 2026 at 07:40 UTC.
Write-TestFile "$Root\claude-code-synced-home\.claude.json" @"
{
  "oauthAccount": { "organizationUuid": "cd-org", "accountUuid": "cd-acct" },
  "cachedGrowthBookFeatures": { "tengu_claudeai_plugins_sync_enabled": true },
  "cachedGrowthBookFeaturesAt": 1790926800000
}
"@
$synced = "$Root\claude-code-synced-home\.claude\plugins\synced\cd-org_cd-acct"
Write-TestFile "$synced\manifest.json" @"
{ "lastUpdated": 1790926800000, "plugins": [{ "name": "cd-throwaway-synced", "generation": 1, "installationPreference": "available" }] }
"@
Write-TestFile "$synced\cd-throwaway-synced~g1.meta.json" @"
{ "server_plugin_id": "cd-throwaway", "marketplace_name": "cd-throwaway", "installation_preference": "available" }
"@
Write-TestFile "$synced\cd-throwaway-synced~g1\.claude-plugin\plugin.json" @"
{ "name": "cd-throwaway-synced", "version": "1.0.0", "description": "Throwaway plugin for testing config-digger." }
"@
Write-TestFile "$synced\cd-throwaway-synced~g1\skills\cd-throwaway-synced-skill\SKILL.md" @"
---
name: cd-throwaway-synced-skill
description: Throwaway skill for testing config-digger. Does nothing.
---
"@

# A second Claude Code home with 30 user servers and 30 Projects, so that the
# Items, the Projects and the Project dropdown scroll in the smallest window,
# and a missing Project with 12 leftovers, so that the What changes of their
# Remove leftovers scrolls in the action dialog.
$many = 1..30 | ForEach-Object { "{0:D2}" -f $_ }
$manyServers = ($many | ForEach-Object { "    `"cd-throwaway-many-$_`": { `"command`": `"npx`", `"args`": [`"-y`", `"cd-throwaway-many-$_`"] }" }) -join ",`n"
$goneServers = (1..12 | ForEach-Object { "{0:D2}" -f $_ } | ForEach-Object { "        `"cd-throwaway-gone-$_`": { `"command`": `"npx`", `"args`": [`"-y`", `"cd-throwaway-gone-$_`"] }" }) -join ",`n"
$manyProjects = (($many | ForEach-Object { "    `"$slash/many/project-$_`": {}" }) + "    `"$slash/many/project-gone`": {`n      `"mcpServers`": {`n$goneServers`n      }`n    }") -join ",`n"
$many | ForEach-Object { New-Item -ItemType Directory -Force "$Root\many\project-$_" | Out-Null }
Write-TestFile "$Root\claude-code-many-home\.claude.json" @"
{
  "mcpServers": {
$manyServers
  },
  "projects": {
$manyProjects
  }
}
"@

# Cursor as the fake backend sees it, with appdata as %APPDATA% and
# claude-code-home as the user's folder: Cursor has run, and its local
# plugin cd-throwaway-kit has a first manifest that does not parse and a
# next one that declares a server, which Cursor reads instead. Step 27.1
# starts the app with both variables set to them.
Write-TestFile "$Root\appdata\Cursor\User\settings.json" "{}`n"
$kit = "$Root\claude-code-home\.cursor\plugins\local\cd-throwaway-kit"
Write-TestFile "$kit\.cursor-plugin\plugin.json" @"
{ "name": "cd-throwaway-kit",, "mcpServers": {} }
"@
Write-TestFile "$kit\.claude-plugin\plugin.json" @"
{
  "name": "cd-throwaway-kit",
  "mcpServers": {
    "cd-throwaway-next": { "command": "npx", "args": ["-y", "cd-throwaway-next"] }
  }
}
"@

# With -VsCode, VS Code's files as they would be in an %APPDATA% of their
# own, vscode-appdata, which the app reads when started with APPDATA set to
# it: the default profile, a profile Work with its own mcp.json, and a
# profile Shared that shares the default profile's MCP servers. VS Code last
# opened the Project in Work and the extra Project in Shared. It also opened
# a workspace file with `,,` after its folders, which it reads around the
# error: it opens the file's folder and skips the file's own servers.
if ($VsCode) {
    $user = "$Root\vscode-appdata\Code\User"
    Write-TestFile "$user\mcp.json" @"
{
  "servers": {
    "cd-throwaway-vscode-default": { "command": "npx", "args": ["-y", "cd-throwaway-vscode-default"] }
  }
}
"@
    Write-TestFile "$user\profiles\-c0ffee01\mcp.json" @"
{
  "servers": {
    "cd-throwaway-vscode-work": { "command": "npx", "args": ["-y", "cd-throwaway-vscode-work"] }
  }
}
"@
    New-Item -ItemType Directory -Force "$user\profiles\-c0ffee02" | Out-Null
    # VS Code keys a workspace by its URI, the drive in lower case and its
    # colon encoded.
    $uri = "file:///" + $Root.Substring(0, 1).ToLower() + "%3A" + ($Root.Substring(2) -replace '\\', '/' -replace ' ', '%20')
    Write-TestFile "$user\globalStorage\storage.json" @"
{
  "userDataProfiles": [
    { "location": "-c0ffee01", "name": "Work" },
    { "location": "-c0ffee02", "name": "Shared", "useDefaultFlags": { "mcp": true } }
  ],
  "profileAssociations": {
    "workspaces": {
      "$uri/project": "-c0ffee01",
      "$uri/project-extra": "-c0ffee02"
    },
    "emptyWindows": {}
  }
}
"@
    Write-TestFile "$Root\vscode-workspace\member\.vscode\mcp.json" @"
{
  "servers": {
    "cd-throwaway-vscode-member": { "command": "npx", "args": ["-y", "cd-throwaway-vscode-member"] }
  }
}
"@
    Write-TestFile "$Root\vscode-workspace\cd-throwaway.code-workspace" @"
{
  "folders": [
    { "path": "member" }
  ],,
  "settings": {
    "mcp": {
      "servers": {
        "cd-throwaway-vscode-workspace": { "command": "npx", "args": ["-y", "cd-throwaway-vscode-workspace"] }
      }
    }
  }
}
"@
    Write-TestFile "$user\workspaceStorage\cd-throwaway-workspace\workspace.json" @"
{
  "workspace": "$uri/vscode-workspace/cd-throwaway.code-workspace"
}
"@
}

# With -Cursor, Cursor's files as they would be in an %APPDATA% of their own,
# cursor-appdata, which the app reads when started with APPDATA set to it:
# Cursor has run, and opened cursor-project, a git repo with a skill in a
# .cursor\skills folder deeper than its own.
if ($Cursor) {
    $user = "$Root\cursor-appdata\Cursor\User"
    Write-TestFile "$user\settings.json" "{}`n"
    # Cursor keys a workspace by its URI, the drive in lower case and its
    # colon encoded.
    $uri = "file:///" + $Root.Substring(0, 1).ToLower() + "%3A" + ($Root.Substring(2) -replace '\\', '/' -replace ' ', '%20')
    Write-TestFile "$user\workspaceStorage\cd-throwaway\workspace.json" @"
{
  "folder": "$uri/cursor-project"
}
"@
    Write-TestFile "$Root\cursor-project\packages\app\.cursor\skills\cd-throwaway-nested-skill\SKILL.md" @"
---
name: cd-throwaway-nested-skill
description: Throwaway skill for testing config-digger. Does nothing.
---
"@
    git init -q "$Root\cursor-project"
    if ($LASTEXITCODE -ne 0) {
        throw "Cannot run git init in $Root\cursor-project: install Git for Windows."
    }
}

# With -Symlink, a skill committed in a repo with git's core.symlinks, as git
# checks it out: its folder outside any Agent home, and a relative folder
# symlink to it in the Claude Code home. Creating a symlink takes Developer
# Mode or admin rights, so the default data has none.
if ($Symlink) {
    Write-TestFile "$Root\linked\cd-throwaway-linked-skill\SKILL.md" @"
---
name: cd-throwaway-linked-skill
description: Throwaway skill for testing config-digger. Does nothing.
---
"@
    # mklink keeps the target as written; New-Item makes it absolute in
    # Windows PowerShell 5.1.
    cmd /c mklink /D "$Root\claude-code-home\.claude\skills\cd-throwaway-linked-skill" "..\..\..\linked\cd-throwaway-linked-skill" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Cannot create the symlink: turn on Developer Mode in Settings > System > For developers, or run PowerShell as an administrator."
    }
}

# With -NameRule, Claude Code skills whose folder names break its name rule,
# one with a leading space and one with a zero-width space, and one with a
# space inside its name, which the rule keeps. The default data has none, so that
# no other step counts them.
if ($NameRule) {
    $skills = "$Root\claude-code-home\.claude\skills"
    foreach ($folder in @(" cd-throwaway-name-lead", "cd-throwaway-name$([char]0x200B)zwsp", "cd-throwaway-name inner")) {
        # `\\?\` keeps a space Windows would otherwise trim from a folder name.
        $path = "\\?\$skills\$folder"
        [IO.Directory]::CreateDirectory($path) | Out-Null
        [IO.File]::WriteAllText("$path\SKILL.md", @"
---
name: cd-throwaway-name-rule
description: Throwaway skill for testing config-digger. Does nothing.
---
"@, [Text.UTF8Encoding]::new($false))
    }
}

# With -PlainString, a Project whose Claude Code settings hold
# `enabledMcpjsonServers` and `disabledMcpjsonServers` as plain strings, which
# Claude Code reads as one-item lists since 2.1.299. Claude Code has not
# recorded it, so that Disable writes its local settings; the tester adds it
# by hand. Nor has the user trusted it, so its own approvals count for
# nothing: the user's settings approve one server by a plain string. The
# default data has none, so that no other step counts it.
if ($PlainString) {
    Write-TestFile "$Root\claude-code-home\.claude\settings.json" @"
{
  "enabledMcpjsonServers": "cd-throwaway-plain-on"
}
"@
    Write-TestFile "$Root\project-plain\.mcp.json" @"
{
  "mcpServers": {
    "cd-throwaway-plain-on": { "command": "npx", "args": ["-y", "cd-throwaway-plain"] },
    "cd-throwaway-plain-off": { "command": "npx", "args": ["-y", "cd-throwaway-plain"] },
    "cd-throwaway-plain-listed": { "command": "npx", "args": ["-y", "cd-throwaway-plain"] },
    "cd-throwaway-plain-other": { "command": "npx", "args": ["-y", "cd-throwaway-plain"] }
  }
}
"@
    Write-TestFile "$Root\project-plain\.claude\settings.json" @"
{
  "enabledMcpjsonServers": ["cd-throwaway-plain-off", "cd-throwaway-plain-listed"]
}
"@
    Write-TestFile "$Root\project-plain\.claude\settings.local.json" @"
{
  "enabledMcpjsonServers": "cd-throwaway-plain-on",
  "disabledMcpjsonServers": "cd-throwaway-plain-off"
}
"@
}

# With -CodexLayer, a git Project Codex trusts, whose `.codex\config.toml`,
# in the git index, declares a server the Codex home does not: Disable
# writes a Stand-in for it in the home's `config.toml`. The default data has
# none, so that no other step counts it.
if ($CodexLayer) {
    $folder = "$Root\codex-project"
    $key = $folder.ToLower()
    $trust = "`n`n[projects.'$key']`ntrust_level = `"trusted`"`n"
    [IO.File]::AppendAllText("$Root\codex-home\config.toml", $trust, [Text.UTF8Encoding]::new($false))
    Write-TestFile "$folder\.codex\config.toml" @"
[mcp_servers.cd-throwaway-codex-layer]
command = "npx"
args = ["-y", "cd-throwaway-codex-layer"]
env = { CD_THROWAWAY_TOKEN = "not-a-real-secret" }
"@
    git init -q $folder
    if ($LASTEXITCODE -ne 0) {
        throw "Cannot run git init in ${folder}: install Git for Windows."
    }
    git -C $folder add .codex/config.toml
    if ($LASTEXITCODE -ne 0) {
        throw "Cannot add .codex\config.toml to the git index of $folder."
    }
}

# With -Frontmatter, Codex skills whose frontmatter is not YAML: one whose
# description holds `: `, which Codex reads quoted, and one with a stray
# line, which it skips. And a Gemini CLI home, gemini-home, whose skills
# Gemini CLI reads line by line: the same two, which it loads, and one with
# no `name` line, which it skips. The default data has none, so that no
# other step counts them.
if ($Frontmatter) {
    $gemini = "$Root\gemini-home\.gemini"
    Write-TestFile "$gemini\settings.json" "{}"
    Write-TestFile "$gemini\skills\cd-throwaway-fm-gemini-colon\SKILL.md" @"
---
name: cd-throwaway-fm-gemini-colon
description: Throwaway skill for testing config-digger. Use when: never.
---
"@
    Write-TestFile "$gemini\skills\cd-throwaway-fm-gemini-stray\SKILL.md" @"
---
name: cd-throwaway-fm-gemini-stray
just text
description: Throwaway skill for testing config-digger. Does nothing.
---
"@
    Write-TestFile "$gemini\skills\cd-throwaway-fm-gemini-noname\SKILL.md" @"
---
description: Throwaway skill for testing config-digger. Use when: never.
---
"@

    $skills = "$Root\codex-home\skills"
    Write-TestFile "$skills\cd-throwaway-fm-colon\SKILL.md" @"
---
name: cd-throwaway-fm-colon
description: Throwaway skill for testing config-digger. Use when: never.
---
"@
    Write-TestFile "$skills\cd-throwaway-fm-stray\SKILL.md" @"
---
name: cd-throwaway-fm-stray
just text
description: Throwaway skill for testing config-digger. Does nothing.
---
"@
}

# With -DesktopOverlay, Claude Desktop's folders as they would be in an
# %APPDATA% and a %LOCALAPPDATA% of their own, desktop-appdata\Roaming and
# desktop-appdata\Local, which the app reads when started with both set to
# them: a classic %APPDATA%\Claude whose config declares a server, and the
# Store package's LocalCache beside it, which holds no config of its own and
# so reads the classic one.
if ($DesktopOverlay) {
    Write-TestFile "$Root\desktop-appdata\Roaming\Claude\claude_desktop_config.json" @"
{
  "mcpServers": {
    "cd-throwaway-overlay": { "command": "npx", "args": ["-y", "cd-throwaway-overlay"] }
  }
}
"@
    Write-TestFile "$Root\desktop-appdata\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\Preferences" "{}`n"
}

Write-Host "Created the throwaway homes and Projects in $Root"
Get-ChildItem $Root -Recurse -File | ForEach-Object { "  " + $_.FullName.Substring($Root.Length + 1) }
