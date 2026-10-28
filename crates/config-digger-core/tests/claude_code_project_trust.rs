//! Claude Code's `.mcp.json` approvals through the core `api`, with fake
//! ports: those of the Project's own settings, local settings and state count
//! only while its state, under its key with forward slashes, holds
//! `hasTrustDialogAccepted: true`, and the user's count in any Project.

use config_digger_core::api::{Context, Core, Inventory, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const PROJECT: &str = r"E:\projects\petme-app";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn in_project(relative: &str) -> PathBuf {
    PathBuf::from(PROJECT).join(relative)
}

/// A Claude Code home whose `~/.claude.json` holds `projects`, and a
/// Project whose `.mcp.json` declares `supabase`.
fn home(projects: Value) -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(
            in_home(".claude.json"),
            json!({ "projects": projects }).to_string(),
        )
        .with_file(
            in_project(".mcp.json"),
            json!({ "mcpServers": {
                "supabase": { "type": "http", "url": "https://mcp.supabase.com/mcp" },
            } })
            .to_string(),
        )
}

/// The `.mcp.json` Entry of `supabase`, in the context of the Project.
fn supabase(fs: &FakeFileSystem) -> Value {
    let core = Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let inventory: Inventory = core.resolve(&snapshot, &context);
    let item = inventory
        .items
        .iter()
        .find(|item| item.name == "supabase")
        .expect("no server `supabase`");
    serde_json::to_value(&item.entries[0]).unwrap()
}

/// `home`, with `supabase` approved by the Project's own `source`: its
/// settings, its local settings or its state, which also holds `state`.
fn approved_by_the_project(source: &str, state: Value) -> FakeFileSystem {
    let approval = json!({ "enabledMcpjsonServers": ["supabase"] });
    let mut project_state = state;
    if source == "state" {
        project_state["enabledMcpjsonServers"] = json!(["supabase"]);
    }
    let fs = home(json!({ "E:/projects/petme-app": project_state }));
    match source {
        "settings" => fs.with_file(in_project(r".claude\settings.json"), approval.to_string()),
        "local settings" => fs.with_file(
            in_project(r".claude\settings.local.json"),
            approval.to_string(),
        ),
        _ => fs,
    }
}

const SOURCES: [&str; 3] = ["settings", "local settings", "state"];

#[test]
fn an_untrusted_projects_own_approvals_do_not_count() {
    for source in SOURCES {
        let entry = supabase(&approved_by_the_project(source, json!({})));

        assert_eq!(entry["state"], "blocked", "{source}");
        assert_eq!(
            entry["reason"],
            json!({
                "code": "not_approved",
                "effective": null,
                "switch": null,
                "sentence": r"Blocked: Claude Code has not approved `supabase` from E:\projects\petme-app\.mcp.json. Approve it in Claude Code, which asks about it when it starts in this Project; the Project's own approvals count only once you trust the Project in Claude Code. config-digger never approves servers, nor trusts a Project.",
            }),
            "{source}"
        );
    }
}

#[test]
fn a_trusted_projects_own_approvals_count() {
    for source in SOURCES {
        let trusted = json!({ "hasTrustDialogAccepted": true });

        let entry = supabase(&approved_by_the_project(source, trusted));

        assert_eq!(entry["state"], "effective", "{source}: {entry}");
    }
}

#[test]
fn trust_and_approvals_under_a_key_with_backslashes_count_for_nothing() {
    let trust = json!({ "hasTrustDialogAccepted": true });
    let approval = json!({ "enableAllProjectMcpServers": true });
    let both = json!({ "hasTrustDialogAccepted": true, "enableAllProjectMcpServers": true });
    let states = [
        json!({ r"E:\projects\petme-app": both }),
        json!({ r"E:\projects\petme-app": trust, "E:/projects/petme-app": approval }),
        json!({ "E:/projects/petme-app": trust, r"E:\projects\petme-app": approval }),
    ];
    for projects in states {
        let entry = supabase(&home(projects.clone()));

        assert_eq!(entry["state"], "blocked", "{projects}");
        assert_eq!(entry["reason"]["code"], "not_approved", "{projects}");
    }
}

#[test]
fn the_users_approvals_count_in_an_untrusted_project() {
    let fs = home(json!({ "E:/projects/petme-app": {} })).with_file(
        in_home(r".claude\settings.json"),
        json!({ "enableAllProjectMcpServers": true }).to_string(),
    );

    let entry = supabase(&fs);

    assert_eq!(entry["state"], "effective", "{entry}");
}

#[test]
fn trust_is_looked_up_without_regard_to_case() {
    let fs = home(json!({
        "e:/PROJECTS/petme-app": { "hasTrustDialogAccepted": true, "enableAllProjectMcpServers": true },
    }));

    let entry = supabase(&fs);

    assert_eq!(entry["state"], "effective", "{entry}");
}
