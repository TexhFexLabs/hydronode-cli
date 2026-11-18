//! C3: help layout, help equivalence, exit codes, error lines.

mod common;
use common::*;

fn help(args: &[&str]) -> String {
    let env = Env::new("https://example.com");
    let out = env.cmd().args(args).output().unwrap();
    assert_eq!(code(&out), 0, "{args:?}: {}", stderr(&out));
    stdout(&out)
}

const COMMANDS: &[(&str, &str)] = &[
    ("login", "Start"),
    ("logout", "Start"),
    ("whoami", "Start"),
    ("status", "Start"),
    ("sensors", "Data"),
    ("values", "Data"),
    ("watch", "Data"),
    ("set", "Data"),
    ("command", "Data"),
    ("export", "Data"),
    ("devices", "Devices"),
    ("console", "Devices"),
    ("push", "Devices"),
    ("simulate", "Devices"),
    ("anomalies", "More"),
    ("messages", "More"),
    ("stations", "More"),
    ("automations", "More"),
    ("tokens", "More"),
    ("sessions", "More"),
    ("api", "More"),
    ("config", "More"),
    ("profile", "More"),
    ("completion", "More"),
    ("open", "More"),
];

#[test]
fn top_level_help_has_groups_one_line_each() {
    let text = help(&["--help"]);
    insta::assert_snapshot!("top_level_help", text);
    let groups = ["Start:", "Data:", "Devices:", "More:"];
    let mut last = 0;
    for g in groups {
        let at = text.find(g).unwrap_or_else(|| panic!("missing {g}"));
        assert!(at > last, "{g} out of order");
        last = at;
    }
    for (cmd, group) in COMMANDS {
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.trim_start().starts_with(&format!("{cmd} ")))
            .collect();
        assert_eq!(lines.len(), 1, "{cmd} should appear exactly once");
        // the command sits under its group
        let pos = text.find(lines[0]).unwrap();
        let group_pos = text.find(&format!("{group}:")).unwrap();
        assert!(pos > group_pos, "{cmd} not under {group}");
    }
    // -h gives the same overview
    assert_eq!(help(&["-h"]), text);
}

#[test]
fn help_cmd_equals_dash_dash_help() {
    for (cmd, _) in COMMANDS {
        assert_eq!(help(&["help", cmd]), help(&[cmd, "--help"]), "{cmd}");
    }
    assert_eq!(
        help(&["help", "sensors", "delete"]),
        help(&["sensors", "delete", "--help"])
    );
}

#[test]
fn every_command_help_has_purpose_options_examples() {
    for (cmd, _) in COMMANDS {
        let text = help(&[cmd, "--help"]);
        let usage = text.find("Usage:").expect("usage");
        let examples = text
            .find("Examples:")
            .unwrap_or_else(|| panic!("{cmd}: no examples"));
        assert!(usage > 0, "{cmd}: purpose comes first");
        assert!(examples > usage, "{cmd}: examples last");
        assert!(text.contains("  hn "), "{cmd}: examples use hn");
        insta::assert_snapshot!(format!("help_{cmd}"), text);
    }
}

#[test]
fn subcommand_help_snapshots() {
    for args in [
        ["sensors", "delete"],
        ["sensors", "credentials"],
        ["values", "compare"],
        ["devices", "update"],
        ["profile", "add"],
        ["messages", "read"],
    ] {
        let text = help(&[args[0], args[1], "--help"]);
        assert!(text.contains("Examples:"), "{args:?}");
        insta::assert_snapshot!(format!("help_{}_{}", args[0], args[1]), text);
    }
}

#[test]
fn hydronode_binary_is_the_same_program() {
    let env = Env::new("https://example.com");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_hydronode"));
    cmd.env("HYDRONODE_CONFIG_DIR", env.dir.path())
        .arg("--version");
    let out = cmd.output().unwrap();
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).starts_with("hydronode "), "{}", stdout(&out));
}

#[test]
fn usage_errors_exit_2() {
    let env = Env::new("https://example.com");
    for args in [
        vec!["frobnicate"],
        vec!["values"],
        vec!["sensors", "rename", "only-one-arg"],
        vec!["completion", "tcsh-nope"],
    ] {
        let out = env.cmd().args(&args).output().unwrap();
        assert_eq!(code(&out), 2, "{args:?}");
    }
}

#[test]
fn not_logged_in_exits_3_with_hint() {
    let env = Env::new("https://example.com");
    let out = env.cmd().args(["sensors", "list"]).output().unwrap();
    assert_eq!(code(&out), 3);
    let err = stderr(&out);
    assert!(err.starts_with("error: not logged in"), "{err}");
    assert!(err.contains("\nhint: run 'hn login'"), "{err}");

    let out = env
        .cmd()
        .args(["sensors", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(code(&out), 3);
    let v: serde_json::Value = serde_json::from_str(&stderr(&out)).unwrap();
    assert_eq!(v["error"]["code"], "NOT_LOGGED_IN");
    assert!(v["error"]["hint"].as_str().unwrap().contains("hn login"));
}

#[test]
fn insecure_profile_server_is_refused() {
    let env = Env::new("http://hydronode.example.com");
    let out = env.cmd().args(["status"]).output().unwrap();
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("plain http"));
}

#[test]
fn version_flag() {
    let out = help(&["--version"]);
    assert!(out.starts_with("hn "), "{out}");
}
