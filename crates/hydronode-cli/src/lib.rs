//! The HydroNode command line (`hydronode`, alias `hn`).

pub mod cli;
pub mod commands;
pub mod ctx;
pub mod output;
pub mod simulate;
pub mod watch;

use std::ffi::OsString;

use clap::FromArgMatches;
use hydronode_api::Error;

/// `hydronode` or `hn`, from how the program was started.
pub fn bin_name(argv0: Option<&OsString>) -> String {
    let stem = argv0
        .and_then(|a| {
            std::path::Path::new(a)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .unwrap_or_default();
    if stem == "hydronode" {
        stem
    } else {
        "hn".into()
    }
}

/// Parse, run, print errors. Returns the exit code.
pub fn run() -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    let bin = bin_name(args.first());
    let matches = match cli::command(&bin).try_get_matches_from(&args) {
        Ok(m) => m,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    let parsed = match cli::Cli::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return 2;
        }
    };
    let json = parsed.json;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    match runtime.block_on(commands::dispatch(parsed, &bin)) {
        Ok(()) => 0,
        Err(e) => {
            print_error(&e, json);
            e.exit_code()
        }
    }
}

pub fn print_error(e: &Error, json: bool) {
    if json {
        let v = serde_json::json!({
            "error": { "code": e.code(), "message": e.to_string(), "hint": e.hint() }
        });
        eprintln!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    eprintln!("error: {e}");
    if let Some(h) = e.hint() {
        eprintln!("hint: {h}");
    }
}
