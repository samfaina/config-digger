# Creates the throwaway Agent homes and Projects for the manual UAT walk of
# config-digger (uat/plan.json, see docs/agents/uat.md). Every Entry is named
# cd-throwaway-* and every Secret is fake (not-a-real-secret). Nothing outside
# -Root is written, and -Root must be a folder called cd-throwaway.
#
#   .\uat\data\cd-throwaway-setup.ps1          creates E:\cd-throwaway
#   .\uat\data\cd-throwaway-setup.ps1 -Reset   deletes E:\cd-throwaway and creates it again
#   .\uat\data\cd-throwaway-setup.ps1 -Root D:\tmp\cd-throwaway -Reset
param(
    [string]$Root = "E:\cd-throwaway",
    [switch]$Reset
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
# Project with one leftover, and a Project to hide.
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

Write-Host "Created the throwaway homes and Projects in $Root"
Get-ChildItem $Root -Recurse -File | ForEach-Object { "  " + $_.FullName.Substring($Root.Length + 1) }
