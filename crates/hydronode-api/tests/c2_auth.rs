//! C2: PKCE + loopback login, device flow, token login, refresh, logout. Fake server: wiremock.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hydronode_api::auth::{self, DeviceEvents, Pkce, WebLoginEvents};
use hydronode_api::client::Client;
use hydronode_api::credentials::{Credential, CredentialStore, StoreKind};
use serde_json::json;
use url::Url;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn token_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "access_token": "eyJ.fake.access2",
        "token_type": "Bearer",
        "expires_in": 3600,
        "refresh_token": "hrt_fake_refresh2"
    }))
}

async fn metadata(server: &MockServer) {
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": base,
            "authorization_endpoint": format!("{base}/oauth/authorize"),
            "token_endpoint": format!("{base}/oauth/token"),
            "device_authorization_endpoint": format!("{base}/oauth/device_authorization"),
            "revocation_endpoint": format!("{base}/oauth/revoke"),
        })))
        .mount(server)
        .await;
}

#[test]
fn pkce_challenge_is_sha256_of_verifier() {
    let pkce = Pkce::new();
    assert!(pkce.verifier.len() >= 43 && pkce.verifier.len() <= 128);
    use base64::Engine;
    use sha2::Digest;
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(pkce.verifier.as_bytes()));
    assert_eq!(pkce.challenge, expected);
    assert_ne!(Pkce::new().verifier, pkce.verifier);
    // RFC 7636 example
    assert_eq!(
        Pkce::challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

/// A fake browser: checks the authorize URL, then calls the loopback callback.
struct FakeBrowser {
    tamper_state: bool,
    /// After the wrong answer, also send the right one (a stale tab first, then the real login).
    then_right: bool,
    /// Open an idle connection first that never sends anything (preconnect, port scanner).
    idle_first: bool,
    seen: Mutex<Option<Url>>,
}

impl WebLoginEvents for FakeBrowser {
    fn open(&self, url: &str) -> bool {
        let url = Url::parse(url).unwrap();
        *self.seen.lock().unwrap() = Some(url.clone());
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let redirect = q["redirect_uri"].clone();
        let right = q["state"].clone();
        let (tamper, then_right, idle_first) =
            (self.tamper_state, self.then_right, self.idle_first);
        std::thread::spawn(move || {
            let _idle = idle_first.then(|| {
                let u = Url::parse(&redirect).unwrap();
                std::net::TcpStream::connect((u.host_str().unwrap(), u.port().unwrap())).unwrap()
            });
            if tamper {
                let _ = fakehttp::blocking_get(&format!("{redirect}?code=evil_code&state=wrong"));
                if !then_right {
                    return;
                }
            }
            let _ = fakehttp::blocking_get(&format!("{redirect}?code=fake_code&state={right}"));
        });
        true
    }
    fn show_url(&self, _url: &str) {}
}

mod fakehttp {
    /// Minimal blocking GET without pulling the blocking feature.
    pub fn blocking_get(url: &str) -> std::io::Result<()> {
        use std::io::{Read, Write};
        let u = url::Url::parse(url).unwrap();
        let mut s = std::net::TcpStream::connect((u.host_str().unwrap(), u.port().unwrap()))?;
        let target = format!("{}?{}", u.path(), u.query().unwrap_or(""));
        write!(
            s,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )?;
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        Ok(())
    }
}

#[tokio::test]
async fn web_login_exchanges_code_with_verifier() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=fake_code"))
        .and(body_string_contains("client_id=hydronode-cli"))
        .and(body_string_contains("code_verifier="))
        .respond_with(token_ok())
        .expect(1)
        .mount(&server)
        .await;

    let browser = FakeBrowser {
        tamper_state: false,
        then_right: false,
        idle_first: false,
        seen: Mutex::new(None),
    };
    let base = Url::parse(&server.uri()).unwrap();
    let cred = auth::login_web(
        &auth::http_client(),
        &base,
        &browser,
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert!(
        matches!(cred, Credential::Session { ref refresh, .. } if refresh == "hrt_fake_refresh2")
    );

    let url = browser.seen.lock().unwrap().clone().unwrap();
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(url.path(), "/oauth/authorize");
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["client_id"], "hydronode-cli");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["code_challenge"].len(), 43);
    assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert!(q["redirect_uri"].ends_with("/callback"));
    assert!(q["state"].len() >= 16);
}

#[tokio::test]
async fn web_login_wrong_state_never_asks_for_a_token() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(token_ok())
        .expect(0)
        .mount(&server)
        .await;
    let browser = FakeBrowser {
        tamper_state: true,
        then_right: false,
        idle_first: false,
        seen: Mutex::new(None),
    };
    let base = Url::parse(&server.uri()).unwrap();
    // a wrong answer is ignored, the login keeps waiting until it times out
    let err = auth::login_web(
        &auth::http_client(),
        &base,
        &browser,
        Duration::from_secs(2),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("timed out"), "{err}");
    assert_eq!(err.exit_code(), 3);
}

#[tokio::test]
async fn web_login_survives_a_wrong_answer_and_an_idle_connection() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("code=fake_code"))
        .respond_with(token_ok())
        .expect(1)
        .mount(&server)
        .await;
    let browser = FakeBrowser {
        tamper_state: true,
        then_right: true,
        idle_first: true,
        seen: Mutex::new(None),
    };
    let base = Url::parse(&server.uri()).unwrap();
    let cred = auth::login_web(
        &auth::http_client(),
        &base,
        &browser,
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    assert!(matches!(cred, Credential::Session { .. }));
}

#[tokio::test]
async fn oversized_answer_is_not_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/me"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(2 * 1024 * 1024)))
        .mount(&server)
        .await;
    let base = Url::parse(&server.uri()).unwrap();
    let err = auth::login_token(&auth::http_client(), &base, "hat_0123456789abcdef")
        .await
        .unwrap_err();
    assert!(matches!(err, hydronode_api::Error::TooLarge(1)), "{err}");
}

#[derive(Default)]
struct RecordingDevice {
    waits: Mutex<Vec<u64>>,
    shown: Mutex<Vec<String>>,
}

impl DeviceEvents for RecordingDevice {
    fn show_code(&self, user_code: &str, uri: &str, _complete: Option<&str>) {
        self.shown
            .lock()
            .unwrap()
            .push(format!("{user_code} {uri}"));
    }
    fn wait(
        &self,
        seconds: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        self.waits.lock().unwrap().push(seconds);
        Box::pin(async {})
    }
}

async fn device_start(server: &MockServer) {
    metadata(server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/device_authorization"))
        .and(body_string_contains("client_id=hydronode-cli"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "fake_device_code",
            "user_code": "BDKM-7QXT",
            "verification_uri": "https://int.example.com/device",
            "verification_uri_complete": "https://int.example.com/device?code=BDKM-7QXT",
            "expires_in": 600,
            "interval": 5
        })))
        .mount(server)
        .await;
}

fn token_error(code: &str) -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_json(json!({"error": code}))
}

#[tokio::test]
async fn device_flow_honors_slow_down_then_succeeds() {
    let server = MockServer::start().await;
    device_start(&server).await;
    // pending, slow_down, then ok
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("device_code=fake_device_code"))
        .respond_with(token_error("authorization_pending"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(token_error("slow_down"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(token_ok())
        .mount(&server)
        .await;

    let events = RecordingDevice::default();
    let base = Url::parse(&server.uri()).unwrap();
    let cred = auth::login_device(&auth::http_client(), &base, &events)
        .await
        .unwrap();
    assert!(cred.is_session());
    assert_eq!(*events.waits.lock().unwrap(), vec![5, 5, 10]);
    assert_eq!(
        events.shown.lock().unwrap()[0],
        "BDKM-7QXT https://int.example.com/device"
    );
}

#[tokio::test]
async fn device_flow_expired_and_denied() {
    for (code, needle) in [("expired_token", "expired"), ("access_denied", "denied")] {
        let server = MockServer::start().await;
        device_start(&server).await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(token_error(code))
            .mount(&server)
            .await;
        let events = RecordingDevice::default();
        let base = Url::parse(&server.uri()).unwrap();
        let err = auth::login_device(&auth::http_client(), &base, &events)
            .await
            .unwrap_err();
        assert!(err.to_string().contains(needle), "{code}: {err}");
        assert_eq!(err.exit_code(), 3);
    }
}

fn me(kind: &str) -> serde_json::Value {
    json!({
        "account": {"name": "Test User", "timeZone": "Europe/Berlin"},
        "auth": {"kind": kind, "scopes": ["hydronode:read"], "projects": "all"},
        "limits": {"readsPerMinute": 120, "writesPerMinute": 20, "maxBuckets": 48, "maxItems": 20}
    })
}

#[tokio::test]
async fn token_login_checks_me_and_keeps_kind() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/me"))
        .and(header("authorization", "Bearer hrt_fake_read"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me("read_pat")))
        .expect(1)
        .mount(&server)
        .await;
    let base = Url::parse(&server.uri()).unwrap();
    let (cred, me) = auth::login_token(&auth::http_client(), &base, "hrt_fake_read")
        .await
        .unwrap();
    assert!(cred.is_read_only());
    assert_eq!(me["auth"]["kind"], "read_pat");

    // garbage is refused before any request
    let err = auth::login_token(&auth::http_client(), &base, "eyJ.not.a.pat")
        .await
        .unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[tokio::test]
async fn token_login_rejected_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/me"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error":"UNAUTHORIZED","message":"Invalid token"})),
        )
        .mount(&server)
        .await;
    let base = Url::parse(&server.uri()).unwrap();
    let err = auth::login_token(&auth::http_client(), &base, "hat_fake_bad")
        .await
        .unwrap_err();
    assert_eq!(err.exit_code(), 3);
}

fn store_with(dir: &std::path::Path, cred: Credential) -> CredentialStore {
    let store = CredentialStore::new(dir, StoreKind::File);
    store.set("default", &cred).unwrap();
    store
}

#[tokio::test]
async fn client_refreshes_expired_session_once_and_saves() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=hrt_fake_refresh1"))
        .respond_with(token_ok())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/sensors"))
        .and(header("authorization", "Bearer eyJ.fake.access2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(2)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let store = store_with(
        dir.path(),
        Credential::Session {
            access: "eyJ.fake.access1".into(),
            refresh: "hrt_fake_refresh1".into(),
            expires_at: 0,
        },
    );
    let client = Client::new(Url::parse(&server.uri()).unwrap(), "default", store.clone());
    client.get("/api/sensors", &[]).await.unwrap();
    client.get("/api/sensors", &[]).await.unwrap();
    match store.get("default").unwrap().unwrap() {
        Credential::Session {
            refresh,
            expires_at,
            ..
        } => {
            assert_eq!(refresh, "hrt_fake_refresh2");
            assert!(expires_at > 1_700_000_000);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn client_refreshes_on_401_and_retries() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(token_ok())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/sensors"))
        .and(header("authorization", "Bearer eyJ.fake.access1"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/sensors"))
        .and(header("authorization", "Bearer eyJ.fake.access2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": "x"}])))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(
        dir.path(),
        Credential::Session {
            access: "eyJ.fake.access1".into(),
            refresh: "hrt_fake_refresh1".into(),
            expires_at: 4_000_000_000,
        },
    );
    let client = Client::new(Url::parse(&server.uri()).unwrap(), "default", store);
    let v = client.get("/api/sensors", &[]).await.unwrap();
    assert_eq!(v[0]["id"], "x");
}

#[tokio::test]
async fn refresh_invalid_grant_ends_session() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(token_error("invalid_grant"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(
        dir.path(),
        Credential::Session {
            access: "eyJ.fake.access1".into(),
            refresh: "hrt_fake_refresh1".into(),
            expires_at: 0,
        },
    );
    let client = Client::new(Url::parse(&server.uri()).unwrap(), "default", store);
    let err = client.get("/api/sensors", &[]).await.unwrap_err();
    assert!(matches!(err, hydronode_api::Error::SessionEnded), "{err:?}");
}

#[tokio::test]
async fn api_error_body_is_parsed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/sensors/nope"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"error": "NOT_FOUND", "message": "Sensor not found"})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(
        dir.path(),
        Credential::Pat {
            token: "hat_fake".into(),
        },
    );
    let client = Client::new(Url::parse(&server.uri()).unwrap(), "default", store);
    let err = client
        .get("/api/agent/v1/sensors/nope", &[])
        .await
        .unwrap_err();
    assert_eq!(err.exit_code(), 4);
    assert_eq!(err.code(), "NOT_FOUND");
    assert_eq!(err.to_string(), "Sensor not found");
}

#[tokio::test]
async fn logout_revokes_and_forgets() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/revoke"))
        .and(body_string_contains("token=hrt_fake_refresh1"))
        .and(body_string_contains("token_type_hint=refresh_token"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/auth/logout"))
        .and(body_string_contains("hrt_fake_refresh1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(
        dir.path(),
        Credential::Session {
            access: "eyJ.fake.access1".into(),
            refresh: "hrt_fake_refresh1".into(),
            expires_at: 0,
        },
    );
    let client = Client::new(Url::parse(&server.uri()).unwrap(), "default", store.clone());
    client.logout().await.unwrap();
    assert!(store.get("default").unwrap().is_none());
}

#[tokio::test]
async fn read_only_token_is_stopped_before_writes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(
        dir.path(),
        Credential::Pat {
            token: "hrt_fake_read".into(),
        },
    );
    let client = Client::new(Url::parse(&server.uri()).unwrap(), "default", store);
    let err = client
        .post("/api/agent/v1/points/p/value", &json!({"value": 1}))
        .await
        .unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.hint().unwrap().contains("hn login"));
}

#[tokio::test]
async fn not_logged_in() {
    let dir = tempfile::tempdir().unwrap();
    let store = CredentialStore::new(dir.path(), StoreKind::File);
    let client = Client::new(Url::parse("https://example.com").unwrap(), "default", store);
    let err = client.get("/api/sensors", &[]).await.unwrap_err();
    assert_eq!(err.exit_code(), 3);
    let _ = Arc::new(());
}

/// int being replaced: Cloudflare answers 530 with an HTML page, no OAuth error body.
#[tokio::test]
async fn device_start_without_oauth_body_names_the_status() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("POST"))
        .and(path("/oauth/device_authorization"))
        .respond_with(ResponseTemplate::new(530).set_body_string("<html>Error 1033</html>"))
        .mount(&server)
        .await;
    let events = RecordingDevice::default();
    let base = Url::parse(&server.uri()).unwrap();
    let err = auth::login_device(&auth::http_client(), &base, &events)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("HTTP 530"), "{err}");
    assert!(err.contains("not reachable"), "{err}");
    assert!(!err.ends_with(": "), "{err}");
}
