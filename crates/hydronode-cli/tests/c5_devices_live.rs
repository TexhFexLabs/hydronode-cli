//! C5: devices, push, simulate, watch, tokens, sessions, api, profiles, login, refresh lock.

mod common;
use common::*;

use serde_json::json;
use wiremock::matchers::{
    body_json, body_partial_json, body_string_contains, header, method, path,
};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SECRET: &str = "fake_secret_for_tests_0123456789";

fn verify_signature(req: &Request) -> bool {
    use hydronode_api::device::sign;
    let body = String::from_utf8_lossy(&req.body);
    let ts = req.headers.get("x-timestamp").unwrap().to_str().unwrap();
    let sig = req.headers.get("x-signature").unwrap().to_str().unwrap();
    sign(SECRET, &format!("{body}{ts}")) == sig
}

#[tokio::test(flavor = "multi_thread")]
async fn push_signs_like_the_sdk() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    Mock::given(method("POST"))
        .and(path("/api/webhook/sensor-values"))
        .and(header("x-sensor-id", SENSOR))
        .and(|req: &Request| verify_signature(req))
        .and(body_string_contains(
            r#""values":[{"type":"TEMPERATURE","value":21.13},{"type":"HUMIDITY","value":48.00}]"#,
        ))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_json(json!({"accepted": 2, "rejected": [], "commands": []})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut cmd = env.cmd();
    cmd.env("HYDRONODE_SENSOR_SECRET", SECRET).args([
        "push",
        SENSOR,
        "TEMPERATURE=21.125",
        "humidity=48",
    ]);
    let out = run(cmd).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("2 accepted"), "{}", stdout(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn push_fetches_secret_when_not_given() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    Mock::given(method("GET"))
        .and(path(format!("/api/sensors/{SENSOR}/credentials")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"sensorId": SENSOR, "secret": SECRET})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/webhook/sensor-values"))
        .and(|req: &Request| verify_signature(req))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"accepted": 1})))
        .expect(1)
        .mount(&server)
        .await;
    let out = run_args(&env, &["push", SENSOR, "TEMPERATURE=20"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!stdout(&out).contains(SECRET) && !stderr(&out).contains(SECRET));

    let out = run_args(&env, &["push", SENSOR, "TEMPERATURE"]).await;
    assert_eq!(code(&out), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn simulate_sends_signed_rounds_and_warns() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    Mock::given(method("POST"))
        .and(path("/api/webhook/sensor-values"))
        .and(|req: &Request| verify_signature(req))
        .and(body_string_contains("TEMPERATURE"))
        .and(body_string_contains("HUMIDITY"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"accepted": 4})))
        .expect(2)
        .mount(&server)
        .await;
    let mut cmd = env.cmd();
    cmd.env("HYDRONODE_SENSOR_SECRET", SECRET)
        .env("HYDRONODE_SIMULATE_FAST", "1")
        .args([
            "simulate", SENSOR, "--kind", "weather", "--every", "10s", "--count", "2",
        ]);
    let out = run(cmd).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("real data"), "{}", stderr(&out));

    let out = run_args(&env, &["simulate", SENSOR, "--every", "5s"]).await;
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("10s"));
}

fn check_body() -> serde_json::Value {
    json!({
        "columns": [{"key": "firmware", "label": "Firmware"}],
        "rows": [
            {"sensorId": SENSOR, "name": "Tank A", "family": "ESP32-C3", "verdict": "OK", "reasons": [],
             "cells": [{"key": "firmware", "label": "Firmware", "type": "CHANGE", "from": "0.8.0", "to": "0.9.0"}],
             "battery": 80, "lowBattery": false},
            {"sensorId": "770e8400-e29b-41d4-a716-446655440000", "name": "Shed", "family": "ESP8266", "verdict": "NOT_POSSIBLE",
             "reasons": ["ESP8266 takes config only"], "cells": [{"key": "firmware", "label": "Firmware", "type": "SKIP", "from": "0.8.0", "to": "0.9.0", "note": "ESP8266 takes config only"}]}
        ],
        "counts": {"ok": 1, "warn": 0, "notPossible": 1}
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_update_checks_then_applies() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    Mock::given(method("POST"))
        .and(path("/api/fleet/v2/changes/check"))
        .and(body_partial_json(json!({"sensorIds": [SENSOR], "change": {"firmwareVersion": "0.9.0", "intervalSeconds": 300}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(check_body()))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/fleet/v2/changes/apply"))
        .and(body_partial_json(json!({"sensorIds": [SENSOR], "confirmLowBattery": false})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"changeId": "c", "number": 143, "label": "B-0143", "jobs": [{"sensorId": SENSOR, "jobId": "j"}]})))
        .expect(1)
        .mount(&server)
        .await;
    let args = [
        "devices",
        "update",
        SENSOR,
        "--firmware",
        "0.9.0",
        "--interval",
        "5m",
    ];
    // non-TTY without --yes: shows the check, then refuses
    let out = run_args(&env, &args).await;
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    insta::assert_snapshot!("devices_check_plain", stdout(&out));

    let mut with_yes = args.to_vec();
    with_yes.push("--yes");
    let out = run_args(&env, &with_yes).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("B-0143"));
}

#[tokio::test(flavor = "multi_thread")]
async fn devices_list() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    Mock::given(method("GET"))
        .and(path("/api/fleet/v2/devices"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"serverTime": "2026-11-17T12:00:00Z", "devices": [
            {"id": SENSOR, "name": "Tank A", "transport": "HTTP", "family": "ESP32-C3", "health": "HEALTHY",
             "firmware": {"product": "HYDRONODE", "version": "0.8.0", "ota": true}, "battery": {"percent": 80},
             "signal": {"rssi": -61, "net": "wifi"}, "updateAvailable": true, "job": null}
        ]})))
        .mount(&server)
        .await;
    let out = run_args(&env, &["devices"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("devices_list_plain", stdout(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn api_passthrough_and_completion() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), pat("hat_fake_full"));
    let me = json!({"account": {"name": "Test User"}, "auth": {"kind": "pat"}});
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/me"))
        .and(header("authorization", "Bearer hat_fake_full"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me.clone()))
        .mount(&server)
        .await;
    let out = run_args(&env, &["api", "GET", "/api/agent/v1/me"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v, me);

    let out = run_args(&env, &["api", "https://evil.example.com/x"]).await;
    assert_eq!(code(&out), 2);

    for shell in ["zsh", "bash", "fish", "powershell"] {
        let out = run_args(&env, &["completion", shell]).await;
        assert_eq!(code(&out), 0);
        assert!(stdout(&out).len() > 500, "{shell}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tokens_and_sessions() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    Mock::given(method("GET"))
        .and(path("/api/user/api-tokens"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "t1", "name": "Home Assistant", "scope": "FULL", "tokenPrefix": "hat_ab12", "createdAt": "2026-10-01T10:00:00Z", "lastUsedAt": null}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/user/api-tokens"))
        .and(body_json(json!({"name": "ci", "scope": "READ"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            json!({"id": "t2", "name": "ci", "token": "hrt_fake_created", "scope": "READ"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/user/sessions/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "s1", "client": "cli", "userAgent": "hydronode-cli/0.1.0", "lastActiveAt": "2026-11-17T11:00:00Z", "expiresAt": "2026-12-17T11:00:00Z", "current": true},
            {"id": "s2", "client": "web", "userAgent": "Mozilla/5.0", "lastActiveAt": "2026-11-16T11:00:00Z", "expiresAt": "2026-12-16T11:00:00Z", "current": false}
        ], "currentKnown": true})))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/user/sessions/v1"))
        .and(wiremock::matchers::query_param("others", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"revoked": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let out = run_args(&env, &["tokens"]).await;
    assert!(stdout(&out).contains("Home Assistant"), "{}", stderr(&out));
    let out = run_args(&env, &["tokens", "create", "ci", "--read"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("hrt_fake_created"));
    let out = run_args(&env, &["sessions"]).await;
    assert!(stdout(&out).contains("HydroNode CLI"), "{}", stdout(&out));
    let out = run_args(&env, &["sessions", "revoke", "--others", "--yes"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn token_login_whoami_logout() {
    let server = MockServer::start().await;
    let env = Env::new(&server.uri());
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "account": {"name": "Test User", "timeZone": "Europe/Berlin"},
            "auth": {"kind": "read_pat", "scopes": ["hydronode:read"], "projects": "all"}})))
        .mount(&server)
        .await;
    let mut cmd = env.cmd();
    cmd.args(["login", "--with-token"])
        .stdin(std::process::Stdio::piped());
    let out = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"hrt_fake_read\n")
            .unwrap();
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!stdout(&out).contains("hrt_fake_read") && !stderr(&out).contains("hrt_fake_read"));
    assert_eq!(env.credentials()["default"]["kind"], "pat");

    let out = run_args(&env, &["whoami", "--json"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["readOnly"], true);
    assert_eq!(v["name"], "Test User");
    assert!(!stdout(&out).contains("hrt_fake_read"));

    let out = run_args(&env, &["logout"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(env.credentials()["default"].is_null());

    // no flag, no TTY: tell the user which flag to use
    let out = run_args(&env, &["login"]).await;
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("--web"));
}

#[tokio::test(flavor = "multi_thread")]
async fn profiles() {
    let server = MockServer::start().await;
    let env = Env::new(&server.uri());
    let out = run_args(
        &env,
        &[
            "profile",
            "add",
            "int",
            "--server",
            "https://int.example.com",
        ],
    )
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let out = run_args(
        &env,
        &["profile", "add", "bad", "--server", "http://example.com"],
    )
    .await;
    assert_eq!(code(&out), 2);
    let out = run_args(&env, &["profile", "list", "--json"]).await;
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["profiles"]["int"]["server"], "https://int.example.com");
    assert_eq!(v["active"], "default");
    let out = run_args(&env, &["-p", "int", "open", "--print"]).await;
    assert_eq!(stdout(&out).trim(), "https://int.example.com/dashboard");
    let out = run_args(&env, &["open", SENSOR, "--print", "-p", "int"]).await;
    assert_eq!(
        stdout(&out).trim(),
        format!("https://int.example.com/sensors/{SENSOR}")
    );
    let out = run_args(&env, &["config", "set", "default-profile", "int"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let out = run_args(&env, &["config", "get", "default-profile"]).await;
    assert_eq!(stdout(&out).trim(), "int");
    let out = run_args(&env, &["-p", "nope", "status"]).await;
    assert_eq!(code(&out), 2);
    let config = std::fs::read_to_string(env.dir.path().join("config.toml")).unwrap();
    assert!(!config.contains("token"));
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_a_profiles_server_drops_its_login() {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    // Same server again: login stays.
    let out = run_args(
        &env,
        &["profile", "add", "default", "--server", &server.uri()],
    )
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!env.credentials()["default"].is_null());
    // Another server: the stored tokens must never be sent there.
    let out = run_args(
        &env,
        &[
            "profile",
            "add",
            "default",
            "--server",
            "https://other.example.com",
        ],
    )
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(env.credentials()["default"].is_null());
    assert!(stdout(&out).contains("signed out"), "{}", stdout(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_processes_refresh_exactly_once() {
    let server = MockServer::start().await;
    let env = Env::with_credential(
        &server.uri(),
        json!({"kind": "session", "access": "eyJ.fake.old", "refresh": "hrt_fake_old", "expiresAt": 0}),
    );
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("refresh_token=hrt_fake_old"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(600))
                .set_body_json(json!({"access_token": "eyJ.fake.new", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "hrt_fake_new"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/sensors"))
        .and(header("authorization", "Bearer eyJ.fake.new"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&server)
        .await;
    let a = {
        let mut c = env.cmd();
        c.args(["sensors", "list"]);
        c
    };
    let b = {
        let mut c = env.cmd();
        c.args(["sensors", "list"]);
        c
    };
    let (oa, ob) = tokio::join!(run(a), run(b));
    assert_eq!(code(&oa), 0, "{}", stderr(&oa));
    assert_eq!(code(&ob), 0, "{}", stderr(&ob));
    assert_eq!(env.credentials()["default"]["refresh"], "hrt_fake_new");
}

/// A fake backend that serves `/api/sensors` over HTTP and `/ws/live/v1` as a WebSocket.
async fn fake_live_server() -> String {
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut peek = [0u8; 64];
                let n = sock.peek(&mut peek).await.unwrap();
                let head = String::from_utf8_lossy(&peek[..n]).to_string();
                if head.contains("/ws/live/v1") {
                    let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
                    let auth = ws.next().await.unwrap().unwrap();
                    assert!(auth.to_text().unwrap().contains("\"auth\""));
                    assert!(auth.to_text().unwrap().contains("eyJ.fake.access"));
                    ws.send(Message::text(r#"{"type":"ready"}"#)).await.unwrap();
                    let watch = ws.next().await.unwrap().unwrap();
                    assert!(watch.to_text().unwrap().contains("watch-dashboard"));
                    ws.send(Message::text(r#"{"type":"watching-dashboard"}"#))
                        .await
                        .unwrap();
                    ws.send(Message::text(format!(
                        r#"{{"type":"reading","sensorId":"{SENSOR}","at":"2026-11-17T12:00:01Z","values":[{{"k":"TEMPERATURE","c":null,"v":21.4}},{{"k":"HUMIDITY","c":null,"v":48}}]}}"#
                    )))
                    .await
                    .unwrap();
                    let _ = ws.close(None).await;
                } else {
                    let mut buf = vec![0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let body = format!(
                        r#"[{{"id":"{SENSOR}","name":"Greenhouse climate","latestValues":[]}}]"#
                    );
                    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = sock.write_all(resp.as_bytes()).await;
                }
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_plain_prints_readings() {
    let base = fake_live_server().await;
    let env = Env::with_credential(&base, session());
    let mut cmd = env.cmd();
    cmd.env("HYDRONODE_WATCH_ONCE", "1")
        .args(["watch", "--plain"]);
    let out = run(cmd).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Greenhouse climate"), "{text}");
    assert!(text.contains("TEMPERATURE"), "{text}");
    assert!(text.contains("21.4"), "{text}");
    assert!(text.contains("HUMIDITY"), "{text}");

    // tokens cannot open the live socket
    let env = Env::with_credential(&base, pat("hat_fake"));
    let out = run_args(&env, &["watch", "--plain"]).await;
    assert_eq!(code(&out), 3);
}
