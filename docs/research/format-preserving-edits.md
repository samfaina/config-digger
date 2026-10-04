# Format-preserving JSONC and TOML edits in Rust

Research for [issue #15](https://github.com/samfaina/config-digger/issues/15). Question: which Rust crates can Remove or Disable a single Entry in a JSON-with-comments (JSONC) file and in a TOML file while keeping comments, key order, whitespace and trailing commas intact?

Researched 2026-10-01. Crate versions and dates come from the crates.io API at that date.

## Recommendation

- **JSONC: `jsonc-parser` (dprint) with the `cst` feature.** It is the only maintained Rust crate with a lossless concrete syntax tree *and* an edit API (`remove`, `set_value`, `append`, `insert`). Parse-then-print is byte-identical, it keeps CRLF and inserts new lines with the file's own newline kind, and it follows the file's trailing-comma style. Deno uses it to edit `deno.json`, and herdr uses it to edit Claude Code and OpenCode settings. Pin the exact version (`=0.34.0`), because the crate is pre-1.0 and minor releases can break the API.
- **TOML: `toml_edit`** (toml-rs). It is the de-facto standard, Codex uses it to edit its own `config.toml`, and parse-then-print is byte-identical for LF files.
- **Diff preview: `similar`** (`TextDiff::from_lines(..).unified_diff()`), with the `inline` feature if the UI needs word-level highlighting.
- **Wrap both libraries in a thin config-digger layer** that handles what the libraries do not:
  1. **Line endings (TOML):** `toml_edit` writes every line ending as LF, so editing a CRLF file without precautions rewrites every line. Detect CRLF, edit an LF copy, then restore CRLF. The spike shows this produces a clean diff.
  2. **BOM:** `jsonc-parser` rejects a UTF-8 BOM, and `toml_edit` accepts one but drops it when printing. Strip the BOM before parsing and put it back after.
  3. **Encoding:** accept only UTF-8, with or without BOM. Refuse UTF-16 and other encodings rather than transcoding them.
  4. **Final newline (TOML):** `toml_edit` adds one when the file has none. Remove it again if the original had none.
  5. **Comment ownership policy:** when an Entry is removed, `jsonc-parser` leaves the comment lines directly above it in place, and `toml_edit` deletes everything above a table header back to the previous item, which can include a section comment that does not belong to the Entry. The spike shows that small helpers fix both cases (J11, T14).
  6. **Edit guard:** before showing a preview, check that every changed line falls inside the Entry's original span plus its attached comments (spans come from `jsonc_parser::parse_to_ast` ranges and `toml_edit::Document` spans). Re-parse the output and compare its value with the expected value. If either check fails, refuse to write.

None of the alternatives should be adopted. `json-strip-comments`, `json_comments`, `serde_jsonc` and `json5` can read JSONC or JSON5 but have no way to write it back with formatting intact. `json-five` has a round-trip mode, but it targets JSON5 and has much less adoption. The Biome (rowan-based) JSON crates have not been published to crates.io since March 2024. Writing a custom rowan CST would duplicate what `jsonc-parser` already provides.

## Candidate evaluation

| Candidate | Format | Can edit with formatting intact? | How a minimal edit is expressed | Maintenance (as of 2026-10-01) | License | Verdict |
|---|---|---|---|---|---|---|
| [`jsonc-parser`](https://crates.io/crates/jsonc-parser) (dprint), `cst` feature | JSONC (also loose JSON5 since 0.34) | **Yes.** Lossless CST: "keeps every comment and every piece of whitespace" ([src/cst/mod.rs](https://github.com/dprint/jsonc-parser/blob/main/src/cst/mod.rs)) | `obj.get("x").unwrap().remove()`; `prop.set_value(json!(true))`; `obj.append("k", json!(v))`; `arr.append(json!("x"))` ([docs.rs](https://docs.rs/jsonc-parser/0.34.0/jsonc_parser/cst/index.html)) | 0.34.0 released 2026-09-27; 1 open issue (from 2022); about 3.3M downloads in the last 90 days ([repo](https://github.com/dprint/jsonc-parser), [crates.io](https://crates.io/crates/jsonc-parser)) | MIT | **Use** |
| [`toml_edit`](https://crates.io/crates/toml_edit) (toml-rs) | TOML 1.1 | **Yes**, with documented limitations: dotted-key order ([#163](https://github.com/toml-rs/toml/issues/163)), missing final newline ([#1158](https://github.com/toml-rs/toml/issues/1158)), CRLF ([#1213](https://github.com/toml-rs/toml/issues/1213)) | `doc["mcp_servers"].as_table_like_mut().unwrap().remove("x")`; `doc["mcp_servers"]["x"]["enabled"] = value(false)` ([docs.rs](https://docs.rs/toml_edit/0.25.15+spec-1.1.0/toml_edit/)) | 0.25.15 released 2026-09-11; repo pushed 2026-10-01; 72 open issues and PRs; about 213M downloads in the last 90 days ([repo](https://github.com/toml-rs/toml)) | MIT OR Apache-2.0 | **Use**. Never call `Table::fmt()` on user files: it drops comments ([#818](https://github.com/toml-rs/toml/issues/818)) |
| [`json-strip-comments`](https://crates.io/crates/json-strip-comments) (oxc) | JSONC, read only | No. It replaces comments and trailing commas with whitespace so `serde_json` can parse the text ([lib.rs](https://github.com/oxc-project/json-strip-comments/blob/main/src/lib.rs)) | n/a | 3.1.2 released 2026-07-27 | Apache-2.0 | Reading only. `jsonc-parser` already covers reading |
| [`json_comments`](https://crates.io/crates/json_comments) | JSONC, read only | No. It is the upstream of `json-strip-comments` ([repo](https://github.com/tmccombs/json-comments-rs)) | n/a | Last release 2023-11-03 | Apache-2.0 | No |
| [`serde_jsonc`](https://crates.io/crates/serde_jsonc) | JSONC, read only | No. It is a fork of `serde_json` (its crate metadata still points at serde-rs/json). Serializing back drops comments | n/a | Last release 2023-10-30 | MIT OR Apache-2.0 | No (stale) |
| [`json5`](https://crates.io/crates/json5) | JSON5 | No. Serde (de)serialization only ([README](https://github.com/callum-oakley/json5-rs)) | n/a | 1.3.1 released 2026-02-07 | MIT | No |
| [`json-five`](https://crates.io/crates/json-five) | JSON5 | Yes. It has a round-trip tokenizer/parser: "Supports round-trip use cases with preservation/editing of whitespace and comments" ([README](https://github.com/spyoungtech/json-five-rs)) | Edits through the model (AST) or token stream (`rt` module) | 0.3.1 released 2026-01-08; 20 stars | MIT | Fallback only. Targets JSON5, less adoption, lower-level API |
| Biome JSON ([`biome_json_parser`](https://crates.io/crates/biome_json_parser) on [`biome_rowan`](https://crates.io/crates/biome_rowan)) | JSON/JSONC | A lossless rowan tree, but it is an internal API with no high-level edit helpers | Mutate syntax nodes by hand | `biome_json_parser` last published to crates.io 2024-03-12 | MIT OR Apache-2.0 | No |
| Custom [`rowan`](https://crates.io/crates/rowan) CST | Any | Possible | We would write the lexer, parser and edit helpers ourselves | rowan 0.17.0 released 2026-09-14 | MIT OR Apache-2.0 | No. This rebuilds `jsonc-parser` |
| [`taplo`](https://crates.io/crates/taplo) | TOML (rowan) | Mainly a formatter and LSP; DOM edits are not its focus | n/a | 0.14.0 released 2025-05-22 | MIT | No. `toml_edit` is the standard |
| **Reference:** VS Code [`jsonc-parser`](https://github.com/microsoft/node-jsonc-parser) (npm) | JSONC | Yes. Text edits on the original string | `modify(text, path, value \| undefined, opts)` returns `Edit[]` (offset, length, content); `applyEdits` splices them in ([edit.ts](https://github.com/microsoft/node-jsonc-parser/blob/main/src/impl/edit.ts)) | Repo pushed 2026-09-29 | MIT | Design reference only (TypeScript) |

### Reference design: VS Code `jsonc-parser` `modify` / `applyEdits`

[`setProperty`](https://github.com/microsoft/node-jsonc-parser/blob/main/src/impl/edit.ts) parses a tree with offsets, finds the target property, and returns one `Edit { offset, length, content }`. Removing a value is `modify(text, path, undefined)`:

- Removing any property except the first deletes from the *end of the previous property* to the end of the removed one. That range takes the separating comma with it, plus any comments and blank lines between the two properties, including comment lines above the removed one.
- Removing the first property deletes from just after `{` up to the start of the next property.
- If `formattingOptions` is passed, the edited line range is re-run through the formatter (`withFormatting`), so inserted text follows the configured indentation and EOL.

Every byte outside the edit stays untouched, so CRLF and BOM preservation come for free. The CST approach in `jsonc-parser` (Rust) reaches the same result because printing an unedited CST reproduces the input exactly. The VS Code model is still the right mental model for the edit guard: an edit is a byte range plus a replacement string, and config-digger can check that range against the Entry's span.

### Prior art: everymcp

[`am-will/everymcp`](https://github.com/am-will/everymcp) is a TypeScript CLI that installs MCP servers into many Agents. Its `package.json` declares MIT, but the repo has no LICENSE file ([package.json](https://github.com/am-will/everymcp/blob/main/package.json)).

- **JSON/JSONC:** VS Code `jsonc-parser` `modify` + `applyEdits` with `{insertSpaces: true, tabSize: 2}` formatting ([config-manager.ts](https://github.com/am-will/everymcp/blob/main/src/core/config-manager.ts)).
- **Preview:** a unified diff from the `diff` npm package (`createPatch(..., { context: 3, stripTrailingCr: true })`) ([diff-engine.ts](https://github.com/am-will/everymcp/blob/main/src/core/diff-engine.ts)).
- **TOML (Codex):** it does not edit the file. It runs `codex mcp list --json` and `codex mcp add` instead ([codex.ts](https://github.com/am-will/everymcp/blob/main/src/agents/codex.ts)).
- Backups go to `~/.everymcp/backups` with a manifest ([backup-manager.ts](https://github.com/am-will/everymcp/blob/main/src/core/backup-manager.ts)).

Other Rust adopters that confirm the choice:

- Codex edits `config.toml` with `toml_edit::DocumentMut` and writes it atomically ([codex-rs/core/src/config/edit.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/config/edit.rs)).
- Deno edits `deno.json` with `jsonc_parser::cst` ([cli/tools/pm/mod.rs](https://github.com/denoland/deno/blob/main/cli/tools/pm/mod.rs)).
- herdr (Apache-2.0) edits Claude Code settings with `CstRootNode` ([src/integration/claude_settings.rs](https://github.com/herdrdev/herdr/blob/master/src/integration/claude_settings.rs)).

## Spike results

The spike is in [`spikes/format-edits/`](../../spikes/format-edits). It pins `jsonc-parser =0.34.0` (feature `cst`), `toml_edit =0.25.15` and `similar =3.2.0`, and was built with cargo 1.97.1 on Windows 11. Every case starts from the original fixture, so each diff shows one edit. Run it with `cargo run` from that directory. The fixtures are [`fixtures/mcp.jsonc`](../../spikes/format-edits/fixtures/mcp.jsonc) (comments, a block comment, trailing commas everywhere, a single-line server object) and [`fixtures/config.toml`](../../spikes/format-edits/fixtures/config.toml) (a Codex-style file with a section comment, per-table comments, an `[mcp_servers.x.env]` sub-table, an array with a trailing comma, and a following non-MCP table).

### Summary

| # | Case | Result |
|---|---|---|
| J0 | JSONC parse + print, no edit | Byte-identical |
| J1 | Remove middle server `github` | Only the property and its comma are removed. **The `/* GitHub ... */` comment above it is left behind** |
| J2 | Remove last server (trailing comma + trailing `// last one`) | Clean: the line is removed together with its trailing comment |
| J3 | Remove first server | Same as J1: the `// Filesystem ...` comment line stays |
| J4 | Toggle existing `"disabled": false` to `true` | One token changed |
| J5 | Add `"disabled": true` to a multi-line object | One line added, indentation matched, **trailing comma added to match the file** |
| J6 | Add a key to a *single-line* object | **The object is expanded to multi-line** (4 lines), and the new last key has no trailing comma |
| J7 | Append to an array (`disabledMcpjsonServers`) | One line added, trailing comma matched |
| J8/J9 | Same edits on a CRLF file | All CRLF kept. Inserted lines use CRLF |
| J10 | UTF-8 BOM | **Parse error** ("Unexpected token on line 1 column 1"). Strip and re-add works |
| J11/J12/J14 | Remove with the attached-comment helper | Entry and its attached comment lines removed. Blank-line separators and other comments kept. Works with CRLF |
| T0 | TOML parse + print, no edit (LF) | Byte-identical |
| T1 | Remove `[mcp_servers.context7]` | The table and its `.env` sub-table removed, **but the section comment `# --- MCP servers ---` is deleted too** (it is stored in the header's prefix decor) |
| T2 | Remove `[mcp_servers.github]` (followed by `[profiles.fast]`) | Clean: the header comment goes with it, and `# Profiles below` stays with the next table |
| T3/T4 | Add `enabled = false`, then flip it | One line added at the end of the table, then one token changed |
| T5 | Remove all servers | No empty `[mcp_servers]` header left behind (the parent is implicit) |
| T6/T7 | Inline-table form (`linear = { ... }` under `[mcp_servers]`) | Clean removal, including the comment line above the key |
| T8 | Add `enabled = false` to an inline table | Works, with odd spacing: `[...] , enabled = false }` |
| T9/T10 | CRLF file, no edit / one edit | **Every line rewritten to LF** |
| T11 | CRLF file, normalize to LF, edit, restore CRLF | Clean diff, all CRLF |
| T12 | UTF-8 BOM | Parses, but **the BOM is dropped on output** |
| T13 | No final newline | **A final `\n` is added** (documented limitation) |
| T14/T15 | Remove with the section-comment helper | Section comment kept, table and its own comment removed |

Round-trips (parse + print, no edit) of public real-world files were all **byte-identical**: three Codex `config.toml` files from GitHub ([feiskyer/codex-settings](https://github.com/feiskyer/codex-settings/blob/main/config.toml), [jacobwgillespie/dotfiles](https://github.com/jacobwgillespie/dotfiles/blob/main/symlinks/.codex/config.toml.symlink), [pulumi/pulumi-awsx](https://github.com/pulumi/pulumi-awsx/blob/master/.codex/config.toml)), Codex's [`defaults.toml`](https://github.com/openai/codex/blob/main/codex-rs/config/defaults.toml), and VS Code's own [`.vscode/settings.json`](https://github.com/microsoft/vscode/blob/main/.vscode/settings.json) (5.9 KB, 20 comment lines). The spike's `roundtrip` subcommand (`cargo run -- roundtrip <files>`) prints only sizes and booleans, never file contents. It was not run against the local user's own Agent configs.

### Selected diffs

All diffs below come from `similar`'s `unified_diff()`, the same output the preview step would show. `\r` marks a CR byte.

**J1: Remove `github` with plain `jsonc-parser` (the comment above is left behind)**

```diff
@@ -9,10 +9,4 @@
 
     /* GitHub: token comes from the environment */
-    "github": {
-      "command": "docker",
-      "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
-      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
-      "disabled": false,
-    },
     "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
   },
```

**J11: Same removal with the attached-comment helper** (about 40 lines on top of `previous_siblings()`; see `jsonc_remove_server_with_comments`)

```diff
@@ -8,11 +8,4 @@
     },
 
-    /* GitHub: token comes from the environment */
-    "github": {
-      "command": "docker",
-      "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
-      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
-      "disabled": false,
-    },
     "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
   },
```

**J2: Remove the last server (trailing comma, trailing comment)**

```diff
@@ -15,5 +15,4 @@
       "disabled": false,
     },
-    "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
   },
   "disabledMcpjsonServers": [
```

**J4 / J5: Toggle a flag / add a missing flag**

```diff
@@ -13,5 +13,5 @@
       "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
       "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
-      "disabled": false,
+      "disabled": true,
     },
```

```diff
@@ -6,4 +6,5 @@
       "command": "npx",
       "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\work"], // only C:\work
+      "disabled": true,
     },
 
```

**J6: Adding a key to a single-line object reformats that object**

```diff
@@ -15,5 +15,9 @@
       "disabled": false,
     },
-    "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
+    "playwright": {
+      "command": "npx",
+      "args": ["@playwright/mcp@latest"],
+      "disabled": true
+    }, // last one
   },
```

**J7: Append to a list**

```diff
@@ -19,4 +19,5 @@
   "disabledMcpjsonServers": [
     "legacy",
+    "github",
   ],
 }
```

**J9: CRLF file, add a flag (the new line uses CRLF too)**

```diff
@@ -6,4 +6,5 @@
       "command": "npx",\r
       "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\work"], // only C:\work\r
+      "disabled": true,\r
     },\r
 \r
```

**T1: Remove `[mcp_servers.context7]` with plain `toml_edit` (the section comment is lost)**

```diff
@@ -2,15 +2,4 @@
 model = "gpt-5-codex"
 
-# --- MCP servers ---
-
-# Docs server, read-only
-[mcp_servers.context7]
-command = "npx"
-args = ["-y", "@upstash/context7-mcp"]  # pinned below
-startup_timeout_sec = 20
-
-[mcp_servers.context7.env]
-LOG_LEVEL = "warn"
-
 # GitHub, uses a PAT from the environment
 [mcp_servers.github]
```

**T14: Same removal with the section-comment helper** (it splits the removed header's prefix decor at the last blank line and moves the first part onto the next table; see `toml_remove_server_keep_section_comment`)

```diff
@@ -4,13 +4,4 @@
 # --- MCP servers ---
 
-# Docs server, read-only
-[mcp_servers.context7]
-command = "npx"
-args = ["-y", "@upstash/context7-mcp"]  # pinned below
-startup_timeout_sec = 20
-
-[mcp_servers.context7.env]
-LOG_LEVEL = "warn"
-
 # GitHub, uses a PAT from the environment
 [mcp_servers.github]
```

**T3: Disable via `enabled = false`** (Codex supports `enabled = false` to "disable without deleting", [Codex MCP docs](https://learn.chatgpt.com/docs/extend/mcp?surface=cli))

```diff
@@ -18,4 +18,5 @@
 bearer_token_env_var = "GH_TOKEN"
 enabled_tools = ["list_issues", "get_issue",]
+enabled = false
 
 # Profiles below
```

**T8: Inline-table form, add `enabled = false`** (odd spacing)

```diff
-sentry = { command = "npx", args = ["@sentry/mcp-server"] }
+sentry = { command = "npx", args = ["@sentry/mcp-server"] , enabled = false }
```

**T10 vs T11: CRLF file, remove `context7`.** Without mitigation, all 23 lines change (`CRLF=23 LF=0` becomes `CRLF=0 LF=12`):

```diff
@@ -1,23 +1,12 @@
-# Codex config\r
-model = "gpt-5-codex"\r
... (every line removed and re-added without \r)
+# Codex config
+model = "gpt-5-codex"
```

With normalize-then-restore (`with_line_endings`), only the Entry's lines change:

```diff
@@ -2,15 +2,4 @@
 model = "gpt-5-codex"\r
 \r
-# --- MCP servers ---\r
-\r
-# Docs server, read-only\r
-[mcp_servers.context7]\r
 ...
 # GitHub, uses a PAT from the environment\r
 [mcp_servers.github]\r
```

## Diff preview

- [`similar`](https://crates.io/crates/similar) 3.2.0 (Apache-2.0, no dependencies, about 53M downloads in the last 90 days, repo pushed 2026-09-28). `TextDiff::from_lines(old, new).unified_diff().context_radius(3).header("a/<path>", "b/<path>")` produces a standard unified diff ([docs.rs `udiff`](https://docs.rs/similar/3.2.0/similar/udiff/index.html)). The spike uses it for every diff above.
- For a GUI preview, render the hunks yourself from `TextDiff::grouped_ops()` / `iter_changes()`. The `inline` feature adds `iter_inline_changes()` for word-level highlights inside a changed line ([Cargo.toml features](https://github.com/mitsuhiko/similar/blob/main/Cargo.toml)).
- Diff the *bytes that will be written*, after BOM and CRLF restoration, not the library's intermediate output. A line-ending regression then shows up as a whole-file diff (as in T10), which the edit guard can catch before anything is written. everymcp passes `stripTrailingCr: true` to its diff library, which hides this kind of regression ([diff-engine.ts](https://github.com/am-will/everymcp/blob/main/src/core/diff-engine.ts)).

## Line endings, BOM and encoding on Windows

- **CRLF is common.** Git for Windows' `core.autocrlf=true` checks text files out with CRLF ([git-config `core.autocrlf`](https://git-scm.com/docs/git-config#Documentation/git-config.txt-coreautocrlf)), and this machine has it set. Project-scope config files in a repository (`.mcp.json`, `.codex/config.toml`, `.vscode/mcp.json`) are therefore often CRLF.
  - `jsonc-parser`: the CST stores each newline's kind (`CstNewlineKind::LineFeed` / `CarriageReturnLineFeed`), and `CstRootNode::newline_kind()` is used for inserted text ([src/cst/mod.rs](https://github.com/dprint/jsonc-parser/blob/main/src/cst/mod.rs)). Confirmed in spike J8/J9/J14.
  - `toml_edit`: prints LF. A uniformly CRLF document comes back uniformly LF, and one with multi-line strings comes back with mixed line endings ([toml-rs/toml#1213](https://github.com/toml-rs/toml/issues/1213), open). Confirmed in T9/T10. Mitigation: if the file is uniformly CRLF, edit an LF copy and convert back (T11). If the file has *mixed* line endings, refuse to edit or ask the user (open question).
- **BOM.** RFC 8259 forbids adding a BOM to JSON but lets parsers ignore one ([RFC 8259 §8.1](https://www.rfc-editor.org/rfc/rfc8259#section-8.1)). Windows PowerShell 5.1 writes a BOM for every Unicode encoding, and `Out-File` / `>` write UTF-16LE ([about_Character_Encoding](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_character_encoding?view=powershell-7.5)). Files touched by scripts may therefore carry a UTF-8 BOM or be UTF-16. `jsonc-parser` fails on a BOM (J10), and `toml_edit` drops it (T12). Mitigation: read bytes, record and strip a UTF-8 BOM, edit, then prepend it again.
- **Encoding.** TOML must be valid UTF-8 ([TOML 1.0 spec](https://toml.io/en/v1.0.0#spec)), and JSON exchanged between systems must be UTF-8 ([RFC 8259 §8.1](https://www.rfc-editor.org/rfc/rfc8259#section-8.1)). Recommendation: decode strictly with `String::from_utf8`. If a UTF-16 BOM is present or the bytes are not valid UTF-8, show the Entry as read-only with a reason instead of transcoding. If transcoding turns out to be needed, [`encoding_rs`](https://crates.io/crates/encoding_rs) is the standard crate ((Apache-2.0 OR MIT) AND BSD-3-Clause).
- **Final newline.** `toml_edit` always ends output with `\n` ([lib.rs Limitations](https://github.com/toml-rs/toml/blob/main/crates/toml_edit/src/lib.rs), [#1158](https://github.com/toml-rs/toml/issues/1158)). Trim it if the original had none (T13).
- **Writing.** Codex writes its config atomically (`write_atomically` in [edit.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/config/edit.rs)), and config-digger should do the same: write a temporary file in the same directory, then rename it over the original. Backups and atomic writes belong to a separate ticket, but the byte-level rules above apply to whatever gets written.

## Sources

- jsonc-parser: [crates.io](https://crates.io/crates/jsonc-parser), [docs.rs `cst`](https://docs.rs/jsonc-parser/0.34.0/jsonc_parser/cst/index.html), [GitHub](https://github.com/dprint/jsonc-parser), [Cargo.toml](https://github.com/dprint/jsonc-parser/blob/main/Cargo.toml), [src/lib.rs](https://github.com/dprint/jsonc-parser/blob/main/src/lib.rs), [src/cst/mod.rs](https://github.com/dprint/jsonc-parser/blob/main/src/cst/mod.rs), [issues](https://github.com/dprint/jsonc-parser/issues?q=is%3Aissue)
- toml_edit: [crates.io](https://crates.io/crates/toml_edit), [docs.rs](https://docs.rs/toml_edit/0.25.15+spec-1.1.0/toml_edit/), [`Document` (spans)](https://docs.rs/toml_edit/0.25.15+spec-1.1.0/toml_edit/struct.Document.html), [GitHub](https://github.com/toml-rs/toml), [lib.rs Limitations](https://github.com/toml-rs/toml/blob/main/crates/toml_edit/src/lib.rs), issues [#163](https://github.com/toml-rs/toml/issues/163), [#818](https://github.com/toml-rs/toml/issues/818), [#1158](https://github.com/toml-rs/toml/issues/1158), [#1213](https://github.com/toml-rs/toml/issues/1213)
- json-strip-comments: [crates.io](https://crates.io/crates/json-strip-comments), [GitHub](https://github.com/oxc-project/json-strip-comments)
- json_comments: [crates.io](https://crates.io/crates/json_comments), [GitHub](https://github.com/tmccombs/json-comments-rs)
- serde_jsonc: [crates.io](https://crates.io/crates/serde_jsonc)
- json5: [crates.io](https://crates.io/crates/json5), [GitHub](https://github.com/callum-oakley/json5-rs)
- json-five: [crates.io](https://crates.io/crates/json-five), [GitHub](https://github.com/spyoungtech/json-five-rs), [docs.rs `rt`](https://docs.rs/json-five/latest/json_five/rt/index.html)
- Biome / rowan / taplo: [biome_json_parser](https://crates.io/crates/biome_json_parser), [biome_rowan](https://crates.io/crates/biome_rowan), [rowan](https://crates.io/crates/rowan), [taplo](https://crates.io/crates/taplo)
- similar: [crates.io](https://crates.io/crates/similar), [docs.rs `udiff`](https://docs.rs/similar/3.2.0/similar/udiff/index.html), [GitHub](https://github.com/mitsuhiko/similar), [CHANGELOG](https://github.com/mitsuhiko/similar/blob/main/CHANGELOG.md)
- encoding_rs: [crates.io](https://crates.io/crates/encoding_rs)
- VS Code jsonc-parser: [GitHub](https://github.com/microsoft/node-jsonc-parser), [src/impl/edit.ts](https://github.com/microsoft/node-jsonc-parser/blob/main/src/impl/edit.ts)
- everymcp: [GitHub](https://github.com/am-will/everymcp), [config-manager.ts](https://github.com/am-will/everymcp/blob/main/src/core/config-manager.ts), [diff-engine.ts](https://github.com/am-will/everymcp/blob/main/src/core/diff-engine.ts), [codex.ts](https://github.com/am-will/everymcp/blob/main/src/agents/codex.ts)
- Adopters: [Codex `config/edit.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/config/edit.rs), [Deno `cli/tools/pm/mod.rs`](https://github.com/denoland/deno/blob/main/cli/tools/pm/mod.rs), [herdr `claude_settings.rs`](https://github.com/herdrdev/herdr/blob/master/src/integration/claude_settings.rs)
- Codex MCP config (`enabled = false`): [learn.chatgpt.com](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
- Encoding and line endings: [RFC 8259 §8.1](https://www.rfc-editor.org/rfc/rfc8259#section-8.1), [TOML 1.0](https://toml.io/en/v1.0.0#spec), [PowerShell about_Character_Encoding](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_character_encoding?view=powershell-7.5), [git-config `core.autocrlf`](https://git-scm.com/docs/git-config#Documentation/git-config.txt-coreautocrlf)

## Open questions

1. **Comment ownership.** Should Remove also delete comment lines directly above an Entry (no blank line in between), and keep section comments separated by a blank line? The spike helpers (J11, T14) implement that rule. The ticket's "touches only the affected Entry" constraint needs an explicit definition of what the Entry owns.
2. **Mixed line endings.** If a file mixes CRLF and LF, should config-digger refuse to edit, or edit and keep the majority ending? `toml_edit` cannot keep them mixed.
3. **Non-UTF-8 files** (UTF-16 from Windows PowerShell 5.1, ANSI code pages): treat as read-only, or transcode with `encoding_rs` and write back in the original encoding?
4. **Single-line objects (J6).** Adding a `disabled` key expands a single-line JSONC object to multiple lines. Is that acceptable, or should Disable avoid adding keys to single-line objects (for example by using an Agent-level disabled list where the Agent has one)? This depends on each Agent's Disable mechanism (a separate ticket).
5. **Upstream fixes.** Should we report the BOM rejection to dprint/jsonc-parser and comment on toml-rs/toml#1213 (CRLF), or just keep the wrapper workarounds? The workarounds are small, so this is not blocking.
6. **jsonc-parser stability.** It is pre-1.0, and 0.33 to 0.34 added JSON5 parsing features. Pin `=0.34.x` and cover the edit helpers with snapshot tests so upgrades are deliberate.
