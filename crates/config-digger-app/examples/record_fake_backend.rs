//! Records the fixtures of the UI's fake backend over the throwaway data in
//! the folder given as the first argument, which it changes as the UAT does,
//! and writes them as TypeScript to the path given as the second.
//! `cargo xtask fake-backend` runs it: it is an example of this crate, rather
//! than code in `xtask`, so that it links with the same Windows runtime as
//! the app.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(root), Some(path), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: record_fake_backend <throwaway root> <path>");
        return ExitCode::from(2);
    };
    let path = std::path::PathBuf::from(path);
    let fixtures = match config_digger_app::fake_backend::record(root.as_ref()) {
        Ok(fixtures) => fixtures,
        Err(error) => {
            eprintln!("cannot record the fixtures: {error}");
            return ExitCode::FAILURE;
        }
    };
    let typescript = config_digger_app::fake_backend::typescript(&fixtures);
    match std::fs::write(&path, typescript) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cannot write {}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}
