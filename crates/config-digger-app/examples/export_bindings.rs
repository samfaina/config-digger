//! Writes the app's TypeScript bindings to the path given as the only argument.
//! `cargo xtask bindings` runs it: it is an example of this crate, rather than
//! code in `xtask`, so that it links with the same Windows runtime as the app.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: export_bindings <path>");
        return ExitCode::from(2);
    };
    match config_digger_app::export_bindings(path.as_ref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cannot export the bindings: {error}");
            ExitCode::FAILURE
        }
    }
}
