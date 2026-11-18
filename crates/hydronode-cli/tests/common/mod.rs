#![allow(dead_code)]

use std::process::Output;

use serde_json::{json, Value};
use tempfile::TempDir;

/// A throwaway config dir with one profile pointing at a fake server.
pub struct Env {
    pub dir: TempDir,
}

impl Env {
    pub fn new(server: &str) -> Env {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            format!("[profiles.default]\nserver = \"{server}\"\n"),
        )
        .unwrap();
        Env { dir }
    }

    pub fn with_credential(server: &str, cred: Value) -> Env {
        let env = Env::new(server);
        env.set_credential(cred);
        env
    }

    pub fn set_credential(&self, cred: Value) {
        std::fs::write(
            self.dir.path().join("credentials.json"),
            serde_json::to_string(&json!({ "default": cred })).unwrap(),
        )
        .unwrap();
    }

    pub fn credentials(&self) -> Value {
        std::fs::read_to_string(self.dir.path().join("credentials.json"))
            .map(|t| serde_json::from_str(&t).unwrap())
            .unwrap_or(Value::Null)
    }

    pub fn cmd(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_hn"));
        cmd.env("HYDRONODE_CONFIG_DIR", self.dir.path())
            .env("HYDRONODE_CREDENTIAL_STORE", "file")
            .env("NO_COLOR", "1")
            .env("HYDRONODE_NO_BROWSER", "1")
            .env_remove("HYDRONODE_PROFILE")
            .env_remove("HYDRONODE_SENSOR_SECRET")
            .env("TZ", "UTC")
            .stdin(std::process::Stdio::null());
        cmd
    }
}

pub fn session() -> Value {
    json!({"kind": "session", "access": "eyJ.fake.access", "refresh": "hrt_fake_refresh", "expiresAt": 4_000_000_000i64})
}

pub fn pat(token: &str) -> Value {
    json!({"kind": "pat", "token": token})
}

/// Run a blocking command without stalling the async runtime that serves the fake server.
pub async fn run(mut cmd: std::process::Command) -> Output {
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

pub async fn run_args(env: &Env, args: &[&str]) -> Output {
    let mut cmd = env.cmd();
    cmd.args(args);
    run(cmd).await
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

pub fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

/// Replace the random wiremock port so snapshots are stable.
pub fn scrub(text: &str, server: &str) -> String {
    text.replace(server, "http://127.0.0.1:PORT")
}

pub const SENSOR: &str = "550e8400-e29b-41d4-a716-446655440000";
pub const POINT: &str = "660e8400-e29b-41d4-a716-446655440000";
