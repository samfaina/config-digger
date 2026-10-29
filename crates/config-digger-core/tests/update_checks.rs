//! What the app keeps of its update checks, and how an update install keeps
//! out of an Operation's way, through the core `api` with fake ports. The
//! check itself is the app's: the core makes no network call.

use config_digger_core::api::{
    Action, Context, Core, Error, ErrorCode, Outcome, ScanOptions, Targets,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{
    self, FakeClock, FakeEnvironment, FakeFileSystem, FakeProcesses,
};
use std::path::PathBuf;
use std::time::Duration;

const HOME: &str = r"C:\Users\fixture";
const APPDATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCALAPPDATA: &str = r"C:\Users\fixture\AppData\Local";

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APPDATA)
        .with_var("LOCALAPPDATA", LOCALAPPDATA)
}

fn core(fs: &FakeFileSystem, clock: &FakeClock) -> Core {
    let mut ports = fake::ports(fs.clone(), env());
    ports.clock = Box::new(clock.clone());
    Core::new(ports).unwrap()
}

fn check_file() -> PathBuf {
    PathBuf::from(LOCALAPPDATA).join(r"config-digger\update-check.json")
}

fn lock_file() -> PathBuf {
    PathBuf::from(LOCALAPPDATA).join(r"config-digger\lock")
}

/// The CLI an update replaces, next to the app.
fn cli() -> PathBuf {
    PathBuf::from(LOCALAPPDATA).join(r"config-digger\config-digger.exe")
}

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

const ONE_USER_SERVER: &str = r#"{
  "mcpServers": {
    "linear": {
      "url": "https://mcp.linear.app/mcp"
    }
  }
}
"#;

#[test]
fn updates_were_never_checked_until_the_app_checks() {
    let fs = FakeFileSystem::new();

    let last = core(&fs, &FakeClock::default())
        .last_update_check()
        .unwrap();

    assert_eq!(last, None);
    assert_eq!(fs.contents(check_file()), None, "reading writes nothing");
}

#[test]
fn the_last_update_check_is_the_time_of_the_latest_one() {
    let fs = FakeFileSystem::new();
    let clock = FakeClock::default();
    core(&fs, &clock).record_update_check().unwrap();
    clock.advance(Duration::from_secs(90 * 60));

    let recorded = core(&fs, &clock).record_update_check().unwrap();

    assert_eq!(recorded, "2026-10-01T13:30:00Z");
    assert_eq!(
        core(&fs, &clock).last_update_check().unwrap().as_deref(),
        Some("2026-10-01T13:30:00Z")
    );
}

#[test]
fn an_update_cannot_install_while_an_operation_holds_the_lock() {
    let fs = FakeFileSystem::new();
    fs.lock_elsewhere(lock_file());

    let error = core(&fs, &FakeClock::default())
        .lock_for_update(&cli())
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::Locked);
}

#[test]
fn no_operation_runs_while_an_update_installs() {
    let fs = FakeFileSystem::new().with_file(claude_json(), ONE_USER_SERVER);
    let clock = FakeClock::default();
    let installing = core(&fs, &clock).lock_for_update(&cli()).unwrap();

    let error = remove_linear(&core(&fs, &clock)).err().unwrap();

    assert_eq!(error.code, ErrorCode::Locked);
    assert_eq!(
        fs.contents(claude_json()).as_deref(),
        Some(ONE_USER_SERVER.as_bytes())
    );
    drop(installing);
}

#[test]
fn an_update_that_does_not_install_lets_operations_run_again() {
    let fs = FakeFileSystem::new().with_file(claude_json(), ONE_USER_SERVER);
    let clock = FakeClock::default();

    drop(core(&fs, &clock).lock_for_update(&cli()).unwrap());

    assert!(remove_linear(&core(&fs, &clock)).is_ok());
}

/// Removes the user server `linear`, as an Operation.
fn remove_linear(core: &Core) -> Result<Outcome, Error> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec!["linear".to_owned()],
        all: false,
        ..Targets::default()
    };
    let plan = core.plan(&snapshot, &Context::default(), Action::Remove, &targets)?;
    core.execute(plan, &[])
}

#[test]
fn an_update_cannot_install_while_the_cli_it_replaces_runs() {
    let fs = FakeFileSystem::new().with_file(claude_json(), ONE_USER_SERVER);
    let clock = FakeClock::default();
    let mut ports = fake::ports(fs.clone(), env());
    ports.clock = Box::new(clock.clone());
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 4242,
        image: "config-digger.exe".to_owned(),
        path: Some(PathBuf::from(
            r"c:\users\fixture\appdata\local\config-digger\CONFIG-DIGGER.EXE",
        )),
        cmdline: "config-digger list".to_owned(),
    }));

    let error = Core::new(ports)
        .unwrap()
        .lock_for_update(&cli())
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(
        error.message.contains(&cli().display().to_string()),
        "{}",
        error.message
    );
    assert!(
        remove_linear(&core(&fs, &clock)).is_ok(),
        "the lock is released"
    );
}
