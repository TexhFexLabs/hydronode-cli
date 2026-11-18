//! Login: browser (authorization code + PKCE S256 over a loopback redirect), device code, token.
//!
//! Contract: `hydronode-backend/docs/api/OAUTH.md`. The CLI is the first-party public client
//! `hydronode-cli`; a successful flow gives a normal session (access JWT + rotating `hrt_` refresh).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use base64::Engine;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

use crate::credentials::Credential;
use crate::{Error, Result};

pub const CLIENT_ID: &str = "hydronode-cli";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The HTTP client every request uses: rustls, no redirects, 30 s timeout.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("hydronode-cli/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("TLS backend available")
}

pub(crate) fn now_secs() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// PKCE pair (RFC 7636, S256).
#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Pkce {
        let verifier = random_token(64);
        let challenge = Pkce::challenge_for(&verifier);
        Pkce {
            verifier,
            challenge,
        }
    }

    pub fn challenge_for(verifier: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Pkce::new()
    }
}

/// Random string from the PKCE "unreserved" alphabet.
pub fn random_token(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// RFC 8414 metadata, with the documented paths as fallback.
#[derive(Debug, Clone)]
pub struct Metadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub device_authorization_endpoint: String,
    pub revocation_endpoint: String,
}

impl Metadata {
    pub fn defaults(server: &Url) -> Metadata {
        let base = server.as_str().trim_end_matches('/').to_string();
        Metadata {
            issuer: base.clone(),
            authorization_endpoint: format!("{base}/oauth/authorize"),
            token_endpoint: format!("{base}/oauth/token"),
            device_authorization_endpoint: format!("{base}/oauth/device_authorization"),
            revocation_endpoint: format!("{base}/oauth/revoke"),
        }
    }

    pub async fn fetch(http: &reqwest::Client, server: &Url) -> Result<Metadata> {
        let mut meta = Metadata::defaults(server);
        let url = server
            .join("/.well-known/oauth-authorization-server")
            .map_err(|e| Error::Other(e.to_string()))?;
        let resp = http.get(url).send().await?;
        if resp.status().as_u16() == 404 {
            return Err(Error::Login(
                "this server does not offer CLI login yet. Use 'hn login --token'".into(),
            ));
        }
        if !resp.status().is_success() {
            return Ok(meta);
        }
        let doc: Value = resp.json().await.unwrap_or(Value::Null);
        let take = |key: &str, slot: &mut String| {
            if let Some(v) = doc[key].as_str() {
                // endpoints must stay on the same origin as the server we trust
                if same_origin(v, server) {
                    *slot = v.to_string();
                }
            }
        };
        // the issuer is an identifier, not an endpoint: compared with `iss` in the callback
        if let Some(iss) = doc["issuer"].as_str() {
            meta.issuer = iss.to_string();
        }
        take("authorization_endpoint", &mut meta.authorization_endpoint);
        take("token_endpoint", &mut meta.token_endpoint);
        take(
            "device_authorization_endpoint",
            &mut meta.device_authorization_endpoint,
        );
        take("revocation_endpoint", &mut meta.revocation_endpoint);
        Ok(meta)
    }
}

fn same_origin(candidate: &str, server: &Url) -> bool {
    Url::parse(candidate)
        .map(|u| u.origin() == server.origin())
        .unwrap_or(false)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Deserialize, Default)]
struct OAuthError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

pub(crate) enum TokenOutcome {
    Ok(Credential),
    Error {
        status: u16,
        code: String,
        description: Option<String>,
    },
}

pub(crate) async fn token_request(
    http: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
) -> Result<TokenOutcome> {
    let resp = http
        .post(endpoint)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if (200..300).contains(&status) {
        let t: TokenResponse = serde_json::from_str(&text)
            .map_err(|_| Error::Login("the server sent an unreadable token answer".into()))?;
        let refresh = t
            .refresh_token
            .ok_or_else(|| Error::Login("the server sent no refresh token".into()))?;
        return Ok(TokenOutcome::Ok(Credential::Session {
            access: t.access_token,
            refresh,
            expires_at: now_secs() + t.expires_in.unwrap_or(3600),
        }));
    }
    let e: OAuthError = serde_json::from_str(&text).unwrap_or_default();
    Ok(TokenOutcome::Error {
        status,
        code: if e.error.is_empty() {
            format!("http_{status}")
        } else {
            e.error
        },
        description: e.error_description,
    })
}

/// What a browser login needs from the outside world. The CLI opens the system browser.
pub trait WebLoginEvents: Send + Sync {
    /// Try to open the URL in a browser. `false` if that did not work.
    fn open(&self, url: &str) -> bool;
    /// Show the URL as a fallback (always called).
    fn show_url(&self, url: &str);
}

/// Browser login: PKCE S256, loopback redirect `http://127.0.0.1:<random port>/callback`.
pub async fn login_web(
    http: &reqwest::Client,
    server: &Url,
    events: &dyn WebLoginEvents,
    timeout: Duration,
) -> Result<Credential> {
    let meta = Metadata::fetch(http, server).await?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let pkce = Pkce::new();
    let state = random_token(32);

    let mut url = Url::parse(&meta.authorization_endpoint)
        .map_err(|e| Error::Login(format!("bad authorization endpoint: {e}")))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", &redirect)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);
    let url = url.to_string();

    events.show_url(&url);
    events.open(&url);

    let params = tokio::time::timeout(timeout, wait_for_callback(&listener, &state, &meta.issuer))
        .await
        .map_err(|_| Error::Login("login timed out, no answer from the browser".into()))??;
    let code = params
        .get("code")
        .ok_or_else(|| Error::Login("the browser returned no code".into()))?;

    match token_request(
        http,
        &meta.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect),
            ("client_id", CLIENT_ID),
            ("code_verifier", &pkce.verifier),
        ],
    )
    .await?
    {
        TokenOutcome::Ok(c) => Ok(c),
        TokenOutcome::Error {
            code, description, ..
        } => Err(Error::Login(format!(
            "login failed: {}",
            description.unwrap_or(code)
        ))),
    }
}

const PAGE: &str = include_str!("login_page.html");

/// The page the browser shows after the redirect back to the CLI. No scripts, nothing loaded from
/// elsewhere (the URL still holds the code), same look as the HydroNode maintenance page.
fn callback_page(ok: bool) -> String {
    let (title, lead) = if ok {
        (
            "Signed in.",
            "Your terminal is connected. You can close this tab.",
        )
    } else {
        (
            "Sign-in failed.",
            "Nothing was connected. The terminal shows why.",
        )
    };
    PAGE.replace("{{TITLE}}", title.trim_end_matches('.'))
        .replace("{{STATE}}", if ok { "ok" } else { "fail" })
        .replace("{{HEADLINE}}", title)
        .replace("{{LEAD}}", lead)
}

async fn wait_for_callback(
    listener: &tokio::net::TcpListener,
    state: &str,
    issuer: &str,
) -> Result<HashMap<String, String>> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = Vec::with_capacity(2048);
        let mut chunk = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
            let n = sock.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let head = String::from_utf8_lossy(&buf);
        let target = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("/")
            .to_string();
        let parsed = Url::parse(&format!("http://127.0.0.1{target}")).ok();
        let Some(parsed) = parsed.filter(|u| u.path() == "/callback") else {
            let _ = sock
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        let outcome = check_callback(&params, state, issuer);
        let page = callback_page(outcome.is_ok());
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\n\
             Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\
             Referrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        let _ = sock.shutdown().await;
        return outcome.map(|_| params);
    }
}

fn check_callback(params: &HashMap<String, String>, state: &str, issuer: &str) -> Result<()> {
    if params.get("state").map(String::as_str) != Some(state) {
        return Err(Error::Login(
            "the browser answer had the wrong state, login stopped".into(),
        ));
    }
    if let Some(iss) = params.get("iss") {
        if iss.trim_end_matches('/') != issuer.trim_end_matches('/') {
            return Err(Error::Login(
                "the browser answer came from another issuer, login stopped".into(),
            ));
        }
    }
    if let Some(err) = params.get("error") {
        return Err(Error::Login(match err.as_str() {
            "access_denied" => "access was denied in the browser".into(),
            other => format!(
                "login failed: {}",
                params
                    .get("error_description")
                    .cloned()
                    .unwrap_or_else(|| other.to_string())
            ),
        }));
    }
    Ok(())
}

/// What the device flow needs from the outside world.
pub trait DeviceEvents: Send + Sync {
    fn show_code(&self, user_code: &str, verification_uri: &str, complete: Option<&str>);
    fn wait(&self, seconds: u64) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

#[derive(Deserialize)]
struct DeviceStart {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    interval: Option<u64>,
}

/// Why the device login could not start. Without an OAuth error body (a proxy page, a host that
/// is restarting) the status code is the only clue, so it is always named.
fn device_start_failure(status: u16, e: OAuthError) -> String {
    let detail = e
        .error_description
        .filter(|d| !d.trim().is_empty())
        .or_else(|| (!e.error.is_empty()).then_some(e.error));
    match detail {
        Some(d) => format!("device login is not available: {d} (HTTP {status})"),
        None if status >= 500 => format!(
            "the server is not reachable right now (HTTP {status}). Try again in a few minutes"
        ),
        None => format!("device login is not available (HTTP {status})"),
    }
}

/// Device code login (RFC 8628): show a code, the user confirms it on the web, we poll.
pub async fn login_device(
    http: &reqwest::Client,
    server: &Url,
    events: &dyn DeviceEvents,
) -> Result<Credential> {
    let meta = Metadata::fetch(http, server).await?;
    let resp = http
        .post(&meta.device_authorization_endpoint)
        .header("Accept", "application/json")
        .form(&[("client_id", CLIENT_ID)])
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let e: OAuthError = resp.json().await.unwrap_or_default();
        return Err(Error::Login(device_start_failure(status, e)));
    }
    let start: DeviceStart = resp
        .json()
        .await
        .map_err(|_| Error::Login("the server sent an unreadable device answer".into()))?;
    events.show_code(
        &start.user_code,
        &start.verification_uri,
        start.verification_uri_complete.as_deref(),
    );
    let deadline = now_secs() + start.expires_in.unwrap_or(600);
    let mut interval = start.interval.unwrap_or(5).max(1);
    loop {
        events.wait(interval).await;
        match token_request(
            http,
            &meta.token_endpoint,
            &[
                ("grant_type", DEVICE_GRANT),
                ("device_code", &start.device_code),
                ("client_id", CLIENT_ID),
            ],
        )
        .await?
        {
            TokenOutcome::Ok(c) => return Ok(c),
            TokenOutcome::Error {
                code, description, ..
            } => match code.as_str() {
                "authorization_pending" => {}
                "slow_down" => interval += 5,
                "access_denied" => {
                    return Err(Error::Login("access was denied in the browser".into()))
                }
                "expired_token" => {
                    return Err(Error::Login(
                        "the code expired, run 'hn login --device' again".into(),
                    ))
                }
                other => {
                    return Err(Error::Login(format!(
                        "login failed: {}",
                        description.unwrap_or_else(|| other.to_string())
                    )))
                }
            },
        }
        if now_secs() > deadline {
            return Err(Error::Login(
                "the code expired, run 'hn login --device' again".into(),
            ));
        }
    }
}

/// Personal access token login: `hat_` full, `hrt_` read only. Checked with `GET /api/agent/v1/me`.
pub async fn login_token(
    http: &reqwest::Client,
    server: &Url,
    token: &str,
) -> Result<(Credential, Value)> {
    let token = token.trim();
    if !(token.starts_with("hat_") || token.starts_with("hrt_")) || token.len() < 8 {
        return Err(Error::Usage(
            "this is not a HydroNode API token (hat_… or hrt_…). Create one under Account › API tokens"
                .into(),
        ));
    }
    let url = server
        .join("/api/agent/v1/me")
        .map_err(|e| Error::Other(e.to_string()))?;
    let resp = http.get(url).bearer_auth(token).send().await?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(crate::client::api_error(status, &text));
    }
    let me: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok((
        Credential::Pat {
            token: token.to_string(),
        },
        me,
    ))
}

#[cfg(test)]
mod page_tests {
    use super::callback_page;

    #[test]
    fn the_callback_page_is_plain_and_loads_nothing() {
        let ok = callback_page(true);
        assert!(ok.contains("Signed in.") && ok.contains("close this tab"));
        assert!(!ok.contains("{{"), "every placeholder filled");
        assert!(!ok.contains("<script") && !ok.contains("http://") && !ok.contains("url("));
        let fail = callback_page(false);
        assert!(fail.contains("class=\"fail\"") && fail.contains("Sign-in failed."));
    }
}
