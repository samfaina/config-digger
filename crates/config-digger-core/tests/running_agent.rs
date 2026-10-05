//! Running-Agent detection and store classes through the core `api`, with a
//! fake process port: the warning or block a write to a location gets while
//! an Agent that reads it runs, and the hints its result adds.

use config_digger_core::api::{
    Action, BlockCode, Context, Core, ErrorCode, Plan, ScanOptions, StoreClass, Targets,
    WarningClass,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{
    self, FakeEnvironment, FakeFileSystem, FakeGitIndex, FakeProcesses,
};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\projects\petme-app";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

/// A user `notes` in `~/.claude.json`, which also registers [`PROJECT`].
const CLAUDE_JSON: &str = r#"{
  "mcpServers": {
    "notes": { "command": "notes-mcp" }
  },
  "projects": {
    "E:/projects/petme-app": {}
  }
}
"#;

/// A project `linear` in [`PROJECT`]'s `.mcp.json`.
const MCP_JSON: &str = r#"{
  "mcpServers": {
    "linear": { "url": "https://mcp.linear.app/mcp" }
  }
}
"#;

fn fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(
            PathBuf::from(HOME).join(r".local\bin\claude.exe"),
            fake::executable(Some([2, 1, 286, 0])),
        )
        .with_file(claude_json(), CLAUDE_JSON)
        .with_file(mcp_json(), MCP_JSON)
}

fn process(pid: u32, image: &str, path: Option<&str>, cmdline: &str) -> RunningProcess {
    RunningProcess {
        pid,
        image: image.to_owned(),
        path: path.map(PathBuf::from),
        cmdline: cmdline.to_owned(),
    }
}

/// The native Claude Code CLI, as it runs from `~/.local/bin`.
fn claude_code_cli() -> RunningProcess {
    process(
        4242,
        "claude.exe",
        Some(r"C:\Users\fixture\.local\bin\claude.exe"),
        r"C:\Users\fixture\.local\bin\claude.exe",
    )
}

/// Claude Desktop, which is also `claude.exe`, from its MSIX package.
fn claude_desktop() -> RunningProcess {
    process(
        5151,
        "claude.exe",
        Some(r"C:\Program Files\WindowsApps\Claude_1.0.0_x64__pzs8sxrjxfjjc\app\claude.exe"),
        "claude.exe",
    )
}

/// A core over `fs` with `running` as the processes on this machine.
fn core(fs: &FakeFileSystem, running: &[RunningProcess]) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.processes = Box::new(
        running
            .iter()
            .fold(FakeProcesses::default(), |processes, process| {
                processes.with_process(process.clone())
            }),
    );
    Core::new(ports).unwrap()
}

fn in_project() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

fn remove(core: &Core, selector: &str) -> Plan {
    let context = in_project();
    let snapshot = core
        .scan(&ScanOptions {
            projects: context.project.iter().cloned().collect(),
        })
        .unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
    };
    core.plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap()
}

fn running_agent_sentence(plan: &Plan) -> Option<String> {
    plan.warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::RunningAgent)
        .map(|warning| warning.sentence.clone())
}

#[test]
fn a_write_to_a_file_the_running_agent_rewrites_gets_a_strong_warning() {
    let fs = fixture();
    let core = core(&fs, &[claude_code_cli()]);

    let plan = remove(&core, "notes");

    let sentence = running_agent_sentence(&plan).expect("a running-agent warning");
    assert!(sentence.contains("Claude Code"), "{sentence}");
    assert!(sentence.contains(".claude.json"), "{sentence}");
    assert!(
        sentence.contains("may overwrite what this Operation writes"),
        "{sentence}"
    );
    let error = core.execute(plan, &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::AckRequired);
    assert_eq!(fs.contents(claude_json()).unwrap(), CLAUDE_JSON.as_bytes());
}

#[test]
fn claude_desktop_is_not_claude_code_though_both_are_claude_exe() {
    let fs = fixture();
    let core = core(&fs, &[claude_desktop()]);

    let plan = remove(&core, "notes");

    assert_eq!(running_agent_sentence(&plan), None);
}

fn session_file(pid: u32) -> PathBuf {
    PathBuf::from(HOME).join(format!(r".claude\sessions\{pid}.json"))
}

#[test]
fn a_session_file_whose_pid_is_alive_means_the_agent_runs() {
    // An npm install runs in `node.exe`, which only its session file names.
    let fs = fixture().with_file(session_file(777), r#"{"peerToken": "s3cr3t"}"#);
    let core = core(&fs, &[process(777, "node.exe", None, "node cli.js")]);

    let plan = remove(&core, "notes");

    assert!(running_agent_sentence(&plan).is_some());
}

#[test]
fn a_session_file_whose_pid_is_gone_is_ignored() {
    let fs = fixture().with_file(session_file(777), "{}").with_file(
        PathBuf::from(HOME).join(r".claude\sessions\888.abc.key"),
        "",
    );
    let core = core(&fs, &[process(888, "node.exe", None, "node")]);

    let plan = remove(&core, "notes");

    assert_eq!(running_agent_sentence(&plan), None);
}

#[test]
fn a_write_to_a_plain_file_the_running_agent_reads_gets_a_warning() {
    let fs = fixture();
    let core = core(&fs, &[claude_code_cli()]);

    let plan = remove(&core, "linear");

    let sentence = running_agent_sentence(&plan).expect("a running-agent warning");
    assert!(sentence.contains(".mcp.json"), "{sentence}");
    assert!(sentence.contains("until it restarts"), "{sentence}");
    assert!(!sentence.contains("overwrite"), "{sentence}");
    assert!(plan.blocks().is_empty());
}

#[test]
fn an_acknowledged_write_adds_a_restart_hint_and_says_it_may_be_overwritten() {
    let fs = fixture();
    let core = core(&fs, &[claude_code_cli()]);
    let plan = remove(&core, "notes");

    let outcome = core.execute(plan, &[WarningClass::RunningAgent]).unwrap();

    assert_eq!(
        outcome.hints,
        [
            "Restart Claude Code so that it reads what the Operation wrote.".to_owned(),
            format!(
                "{} may be overwritten while Claude Code runs.",
                claude_json().display()
            ),
        ]
    );
    assert!(
        !fs.contents(claude_json())
            .unwrap()
            .windows(5)
            .any(|w| w == b"notes")
    );
}

#[test]
fn a_write_to_a_plain_file_adds_only_the_restart_hint() {
    let fs = fixture();
    let core = core(&fs, &[claude_code_cli()]);
    let plan = remove(&core, "linear");

    let outcome = core.execute(plan, &[WarningClass::RunningAgent]).unwrap();

    assert_eq!(
        outcome.hints,
        ["Restart Claude Code so that it reads what the Operation wrote."]
    );
}

#[test]
fn with_no_agent_running_there_is_no_warning_and_no_hint() {
    let fs = fixture();
    let core = core(&fs, &[]);
    let plan = remove(&core, "notes");
    assert!(plan.warnings().is_empty());

    let outcome = core.execute(plan, &[]).unwrap();

    assert!(outcome.hints.is_empty());
}

/// An Agent whose servers live in a store it holds open in SQLite, read
/// here as JSON: only its store class matters.
const SQLITE_AGENT: &str = r#"
format_version = 1

[agent]
id = "store-agent"
name = "Store Agent"

[[installations]]
id = "default"
folder = "~/.store"
executable = "store.exe"
versions = [{ block = "exe-version" }]

[process]
images = ["store.exe"]

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".store/state.vscdb"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "state" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "sqlite"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

const STORE: &str = r#"{"servers": {"search": {"command": "search-mcp"}}}"#;

fn store() -> PathBuf {
    PathBuf::from(HOME).join(r".store\state.vscdb")
}

/// [`SQLITE_AGENT`]'s store, and its Installation.
fn store_fs() -> FakeFileSystem {
    FakeFileSystem::new().with_file(store(), STORE).with_file(
        PathBuf::from(HOME).join(r".store\store.exe"),
        fake::executable(None),
    )
}

/// A core over `fs` with `definition` loaded besides the built-in ones and `processes` as the
/// processes on this machine.
fn core_with(fs: &FakeFileSystem, definition: &str, processes: &FakeProcesses) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.processes = Box::new(processes.clone());
    Core::with_extra_definitions(ports, &[("test.toml", definition)]).unwrap()
}

fn store_agent() -> RunningProcess {
    process(
        31,
        "store.exe",
        Some(r"C:\Program Files\Store\store.exe"),
        "store.exe",
    )
}

#[test]
fn a_write_to_a_sqlite_store_while_its_agent_runs_is_blocked_whatever_is_acknowledged() {
    let fs = store_fs();
    let processes = FakeProcesses::default().with_process(store_agent());
    let core = core_with(&fs, SQLITE_AGENT, &processes);

    let plan = remove(&core, "search");

    let [block] = plan.blocks() else {
        panic!("one block: {:?}", plan.blocks());
    };
    assert_eq!(block.code, BlockCode::RunningAgentSqlite);
    assert!(
        block.sentence.contains("Close Store Agent and retry"),
        "{}",
        block.sentence
    );
    let every_class = [
        WarningClass::GitVersioned,
        WarningClass::WideReach,
        WarningClass::RunningAgent,
        WarningClass::NeedsAttention,
    ];
    let error = core.execute(plan, &every_class).unwrap_err();
    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(
        error.message.contains("Close Store Agent and retry"),
        "{}",
        error.message
    );
    assert_eq!(fs.contents(store()).unwrap(), STORE.as_bytes());
}

/// The store class of each file the plan writes, as its preview tags it.
fn store_classes(plan: &Plan) -> Vec<(PathBuf, StoreClass)> {
    plan.preview()
        .files
        .iter()
        .map(|file| (file.path.clone(), file.store_class))
        .collect()
}

#[test]
fn the_preview_tags_each_file_with_its_store_class_whether_or_not_its_agent_runs() {
    let fs = fixture();
    let core = core(&fs, &[]);
    assert_eq!(
        store_classes(&remove(&core, "notes")),
        [(claude_json(), StoreClass::RewrittenWhileRunning)]
    );
    assert_eq!(
        store_classes(&remove(&core, "linear")),
        [(mcp_json(), StoreClass::Plain)]
    );

    let fs = store_fs();
    let core = core_with(&fs, SQLITE_AGENT, &FakeProcesses::default());
    assert_eq!(
        store_classes(&remove(&core, "search")),
        [(store(), StoreClass::Sqlite)]
    );
}

#[test]
fn a_write_to_a_sqlite_store_goes_ahead_once_its_agent_is_closed() {
    let fs = store_fs();
    let core = core_with(&fs, SQLITE_AGENT, &FakeProcesses::default());

    let plan = remove(&core, "search");
    assert!(plan.blocks().is_empty());
    assert!(plan.warnings().is_empty());

    core.execute(plan, &[]).unwrap();
}

#[test]
fn an_agent_that_starts_after_the_preview_still_blocks_its_sqlite_store() {
    let fs = store_fs();
    let processes = FakeProcesses::default();
    let core = core_with(&fs, SQLITE_AGENT, &processes);
    let plan = remove(&core, "search");

    processes.start(store_agent());
    let error = core.execute(plan, &[]).unwrap_err();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert_eq!(fs.contents(store()).unwrap(), STORE.as_bytes());
}

/// [`SQLITE_AGENT`] with a plain store and `process` in place of its own.
fn plain_agent(process: &str) -> String {
    SQLITE_AGENT
        .replace("[process]\nimages = [\"store.exe\"]\n", process)
        .replace("store_class = \"sqlite\"", "store_class = \"plain\"")
}

#[test]
fn every_cmdline_text_must_match() {
    // Gemini CLI runs in `node.exe`.
    let definition = plain_agent(
        "[process]\nimages = [\"node.exe\"]\ncmdline_contains = [\"gemini-cli\", \"gemini.js\"]\n",
    );
    let fs = store_fs();
    let processes = FakeProcesses::default().with_process(process(
        40,
        "node.exe",
        None,
        r"node C:\npm\node_modules\@google\gemini-cli\index.js",
    ));
    let core = core_with(&fs, &definition, &processes);
    assert_eq!(running_agent_sentence(&remove(&core, "search")), None);

    processes.start(process(
        41,
        "NODE.EXE",
        None,
        r"node C:\npm\node_modules\@google\Gemini-CLI\dist\gemini.js",
    ));

    assert!(running_agent_sentence(&remove(&core, "search")).is_some());
}

#[test]
fn a_home_may_override_its_agents_process() {
    let definition = plain_agent("[process]\nimages = [\"classic.exe\"]\n").replace(
        "roots.state = ",
        "process = { images = [\"msix.exe\"] }\nroots.state = ",
    );
    let fs = store_fs();
    let processes = FakeProcesses::default().with_process(process(50, "classic.exe", None, ""));
    let core = core_with(&fs, &definition, &processes);
    assert_eq!(running_agent_sentence(&remove(&core, "search")), None);

    processes.start(process(51, "msix.exe", None, ""));

    let sentence = running_agent_sentence(&remove(&core, "search")).expect("a warning");
    assert!(sentence.contains("Store Agent"), "{sentence}");
}

#[test]
fn an_undo_while_the_agent_runs_gets_the_warning_too() {
    let fs = fixture();
    let processes = FakeProcesses::default();
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.processes = Box::new(processes.clone());
    let core = Core::new(ports).unwrap();
    let outcome = core.execute(remove(&core, "notes"), &[]).unwrap();

    processes.start(claude_code_cli());
    let undo = core.plan_undo(&outcome.operation).unwrap();

    let sentence = running_agent_sentence(&undo).expect("a running-agent warning");
    assert!(
        sentence.contains("may overwrite what this Operation writes"),
        "{sentence}"
    );
}

#[test]
fn a_block_wins_over_a_missing_acknowledgement() {
    let fs = store_fs();
    let processes = FakeProcesses::default().with_process(store_agent());
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.processes = Box::new(processes);
    ports.git = Box::new(FakeGitIndex::default().with_versioned(store()));
    let core = Core::with_extra_definitions(ports, &[("test.toml", SQLITE_AGENT)]).unwrap();
    let plan = remove(&core, "search");
    assert_eq!(plan.warnings()[0].class, WarningClass::GitVersioned);

    let error = core.execute(plan, &[]).unwrap_err();

    assert_eq!(error.code, ErrorCode::Blocked);
}

#[test]
fn an_agent_that_starts_after_the_preview_needs_a_fresh_preview() {
    let fs = fixture();
    let processes = FakeProcesses::default();
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.processes = Box::new(processes.clone());
    let core = Core::new(ports).unwrap();
    let plan = remove(&core, "notes");
    assert!(plan.warnings().is_empty());

    processes.start(claude_code_cli());
    let error = core.execute(plan, &[]).unwrap_err();

    assert_eq!(error.code, ErrorCode::Stale);
    assert!(error.message.contains("Claude Code"), "{}", error.message);
    assert_eq!(fs.contents(claude_json()).unwrap(), CLAUDE_JSON.as_bytes());
}

#[test]
fn an_agent_running_for_two_of_its_homes_gets_one_hint() {
    let definition = plain_agent("[process]\nimages = [\"store.exe\"]\n").replace(
        "[[scopes]]",
        "[[homes]]\nid = \"other\"\nfolder = { block = \"path\", path = \"~\" }\nroots.state = \".other/state.vscdb\"\n\n[[scopes]]",
    );
    let other = PathBuf::from(HOME).join(r".other\state.vscdb");
    let fs = store_fs().with_file(&other, STORE);
    let processes = FakeProcesses::default().with_process(store_agent());
    let core = core_with(&fs, &definition, &processes);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![
            format!("{}#servers.search", store().display()),
            format!("{}#servers.search", other.display()),
        ],
        all: false,
    };
    let plan = core
        .plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap();

    let outcome = core.execute(plan, &[WarningClass::RunningAgent]).unwrap();

    assert_eq!(
        outcome.hints,
        ["Restart Store Agent so that it reads what the Operation wrote."]
    );
}

#[test]
fn without_an_installation_a_running_agent_still_blocks_its_sqlite_store_but_never_warns() {
    let fs = FakeFileSystem::new().with_file(store(), STORE);
    let processes = FakeProcesses::default().with_process(store_agent());
    let core = core_with(
        &fs,
        &plain_agent("[process]\nimages = [\"store.exe\"]\n"),
        &processes,
    );
    assert_eq!(running_agent_sentence(&remove(&core, "search")), None);
    let core = core_with(&fs, SQLITE_AGENT, &processes);

    let plan = remove(&core, "search");

    let codes: Vec<BlockCode> = plan.blocks().iter().map(|block| block.code).collect();
    assert_eq!(codes, [BlockCode::RunningAgentSqlite]);
    assert_eq!(plan.warnings(), []);
}
