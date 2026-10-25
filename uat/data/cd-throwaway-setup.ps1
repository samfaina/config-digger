# Creates the throwaway Agent homes and Projects for the manual UAT walk of
# config-digger (uat/plan.json, see docs/agents/uat.md). Every Entry is named
# cd-throwaway-* and every Secret is fake (not-a-real-secret). Nothing outside
# -Root is written, and -Root must be a folder called cd-throwaway.
#
#   .\uat\data\cd-throwaway-setup.ps1          creates E:\cd-throwaway
#   .\uat\data\cd-throwaway-setup.ps1 -Reset   deletes E:\cd-throwaway and creates it again
#   .\uat\data\cd-throwaway-setup.ps1 -Root D:\tmp\cd-throwaway -Reset
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -Symlink   adds a skill behind a relative folder symlink
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -VsCode    adds VS Code profiles in vscode-appdata
#   .\uat\data\cd-throwaway-setup.ps1 -Reset -Cursor    adds Cursor in cursor-appdata and a git Project with a nested skill
param(
    [string]$Root = "E:\cd-throwaway",
    [switch]$Reset,
    [switch]$Symlink,
    [switch]$VsCode,
    [switch]$Cursor
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

# Claude Code home: a user server, a Project with a local server, a missing
# Project with one leftover, a missing Project with nothing but a trust, as
# Claude Code keeps for a deleted worktree, and a Project to hide.
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
    "$slash/project-hide": {}
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

# A Project to hide, and one that no registry knows, to add by hand.
New-Item -ItemType Directory -Force "$Root\project-hide" | Out-Null
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

# Claude Desktop home: Disable parks its servers, which the uninstall test uses.
Write-TestFile "$Root\claude-desktop-home\claude_desktop_config.json" @"
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
    "$slash/project": { "mcpServers": [] }
  }
}
"@

# A Claude Code home with a plugin synced from claude.ai, which Claude Code
# keeps in a folder per account and names by its manifest, and the plugin
# sync gate cached on, on 2 Oct 2026 at 07:40 UTC.
Write-TestFile "$Root\claude-code-synced-home\.claude.json" @"
{
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

# With -VsCode, VS Code's files as they would be in an %APPDATA% of their
# own, vscode-appdata, which the app reads when started with APPDATA set to
# it: the default profile, a profile Work with its own mcp.json, and a
# profile Shared that shares the default profile's MCP servers. VS Code last
# opened the Project in Work and the extra Project in Shared.
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

Write-Host "Created the throwaway homes and Projects in $Root"
Get-ChildItem $Root -Recurse -File | ForEach-Object { "  " + $_.FullName.Substring($Root.Length + 1) }
