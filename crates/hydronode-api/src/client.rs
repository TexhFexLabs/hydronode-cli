//! Authenticated HTTP client with refresh under the profile lock.

use reqwest::Method;
use serde_json::Value;
use tokio::sync::Mutex;
use url::Url;

use crate::auth::{self, now_secs, Metadata, TokenOutcome, CLIENT_ID};
use crate::credentials::{Credential, CredentialStore};
use crate::{Error, Result};

/// Refresh this long before the access token runs out.
const REFRESH_EARLY_SECS: i64 = 60;

/// Raw answer of [`Client::request`].
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub fn json(&self) -> Value {
        if self.body.is_empty() {
            return Value::Null;
        }
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&self.body).into_owned()))
    }
}

pub struct Client {
    base: Url,
    http: reqwest::Client,
    profile: String,
    store: CredentialStore,
    cached: Mutex<Option<Credential>>,
}

/// `{"error": "CODE", "message": "…"}` (or a plain text error) into [`Error::Api`].
pub fn api_error(status: u16, body: &str) -> Error {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error_field = v["error"].as_str().map(str::to_string);
    let looks_like_code = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    let code = error_field.clone().filter(|e| looks_like_code(e));
    let message = v["message"]
        .as_str()
        .map(str::to_string)
        .or_else(|| error_field.filter(|e| !looks_like_code(e)))
        .or_else(|| v["error_description"].as_str().map(str::to_string))
        .unwrap_or_else(|| match status {
            401 => "not signed in or the login expired".into(),
            403 => "not allowed".into(),
            404 => "not found".into(),
            429 => "too many requests".into(),
            s => format!("the server answered {s}"),
        });
    Error::Api {
        status,
        code,
        message,
    }
}

impl Client {
    pub fn new(base: Url, profile: &str, store: CredentialStore) -> Client {
        Client {
            base,
            http: auth::http_client(),
            profile: profile.to_string(),
            store,
            cached: Mutex::new(None),
        }
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn store(&self) -> &CredentialStore {
        &self.store
    }

    /// The stored credential (no refresh).
    pub async fn credential(&self) -> Result<Credential> {
        let mut cached = self.cached.lock().await;
        if let Some(c) = cached.as_ref() {
            return Ok(c.clone());
        }
        let c = self
            .store
            .get(&self.profile)?
            .ok_or_else(|| Error::NotLoggedIn {
                profile: self.profile.clone(),
            })?;
        *cached = Some(c.clone());
        Ok(c)
    }

    /// The credential, refreshed first when the access token is (nearly) expired.
    pub async fn fresh_credential(&self) -> Result<Credential> {
        let c = self.credential().await?;
        if let Credential::Session { expires_at, .. } = &c {
            if *expires_at <= now_secs() + REFRESH_EARLY_SECS {
                return self.refresh(&c).await;
            }
        }
        Ok(c)
    }

    /// A short-lived access JWT (live socket). Tokens cannot open it.
    pub async fn access_token(&self) -> Result<String> {
        match self.fresh_credential().await? {
            Credential::Session { access, .. } => Ok(access),
            Credential::Pat { .. } => Err(Error::NeedsFullLogin(
                "live updates need a browser or device login, API tokens cannot open them".into(),
            )),
        }
    }

    /// Refresh under the profile lock. Reads the store again after the lock: when another
    /// process already refreshed, its result is used and nothing is sent.
    async fn refresh(&self, used: &Credential) -> Result<Credential> {
        let store = self.store.clone();
        let profile = self.profile.clone();
        let guard = tokio::task::spawn_blocking(move || store.lock(&profile))
            .await
            .map_err(|e| Error::Other(e.to_string()))??;

        let current = self
            .store
            .get(&self.profile)?
            .ok_or_else(|| Error::NotLoggedIn {
                profile: self.profile.clone(),
            })?;
        let refresh = match &current {
            Credential::Session {
                access,
                expires_at,
                refresh,
            } => {
                if access != used.bearer() && *expires_at > now_secs() + REFRESH_EARLY_SECS {
                    *self.cached.lock().await = Some(current.clone());
                    drop(guard);
                    return Ok(current);
                }
                refresh.clone()
            }
            Credential::Pat { .. } => {
                drop(guard);
                return Ok(current);
            }
        };

        let meta = Metadata::fetch(&self.http, &self.base)
            .await
            .unwrap_or_else(|_| Metadata::defaults(&self.base));
        let outcome = auth::token_request(
            &self.http,
            &meta.token_endpoint,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", CLIENT_ID),
            ],
        )
        .await?;
        let result = match outcome {
            TokenOutcome::Ok(fresh) => {
                self.store.set(&self.profile, &fresh)?;
                *self.cached.lock().await = Some(fresh.clone());
                Ok(fresh)
            }
            TokenOutcome::Error { code, status, .. }
                if code == "invalid_grant" || status == 401 =>
            {
                self.store.delete(&self.profile)?;
                *self.cached.lock().await = None;
                Err(Error::SessionEnded)
            }
            TokenOutcome::Error {
                code,
                description,
                status,
            } => Err(Error::Api {
                status,
                code: Some(code.to_uppercase()),
                message: description.unwrap_or_else(|| "could not refresh the session".into()),
            }),
        };
        drop(guard);
        result
    }

    fn url(&self, path: &str, query: &[(&str, String)]) -> Result<Url> {
        let mut url = self
            .base
            .join(path)
            .map_err(|e| Error::Usage(format!("bad path '{path}': {e}")))?;
        if url.origin() != self.base.origin() {
            return Err(Error::Usage(format!(
                "'{path}' points to another host, use a path like /api/sensors"
            )));
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(url)
    }

    /// Send a request with the stored credential. Refreshes once on 401.
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<RawResponse> {
        let url = self.url(path, query)?;
        let mut cred = self.fresh_credential().await?;
        if method != Method::GET && method != Method::HEAD && cred.is_read_only() {
            return Err(Error::NeedsFullLogin(
                "a read-only token (hrt_) can only read".into(),
            ));
        }
        let mut retried = false;
        loop {
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(cred.bearer())
                .header("Accept", "application/json");
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = req.send().await?;
            let status = resp.status().as_u16();
            if status == 401 && cred.is_session() && !retried {
                retried = true;
                cred = self.refresh(&cred).await?;
                continue;
            }
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = resp.bytes().await?.to_vec();
            return Ok(RawResponse {
                status,
                content_type,
                body: bytes,
            });
        }
    }

    /// Like [`Client::request`], but errors on non-2xx and parses JSON.
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let raw = self.request(method, path, query, body).await?;
        if !(200..300).contains(&raw.status) {
            return Err(api_error(raw.status, &String::from_utf8_lossy(&raw.body)));
        }
        Ok(raw.json())
    }

    pub async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.send(Method::GET, path, query, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(Method::POST, path, &[], Some(body)).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(Method::PUT, path, &[], Some(body)).await
    }

    pub async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(Method::PATCH, path, &[], Some(body)).await
    }

    pub async fn delete(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.send(Method::DELETE, path, query, None).await
    }

    /// End the session on the server (refresh family revoked) and forget the credential.
    /// API tokens are only forgotten here; revoke them on the web or with `hn tokens revoke`.
    pub async fn logout(&self) -> Result<bool> {
        let cred = match self.store.get(&self.profile)? {
            Some(c) => c,
            None => return Ok(false),
        };
        if let Credential::Session { refresh, .. } = &cred {
            let meta = Metadata::fetch(&self.http, &self.base)
                .await
                .unwrap_or_else(|_| Metadata::defaults(&self.base));
            let _ = self
                .http
                .post(&meta.revocation_endpoint)
                .form(&[
                    ("token", refresh.as_str()),
                    ("token_type_hint", "refresh_token"),
                    ("client_id", CLIENT_ID),
                ])
                .send()
                .await;
            if let Ok(url) = self.url("/api/auth/logout", &[]) {
                let _ = self
                    .http
                    .post(url)
                    .json(&serde_json::json!({ "refreshToken": refresh }))
                    .send()
                    .await;
            }
        }
        self.store.delete(&self.profile)?;
        *self.cached.lock().await = None;
        Ok(true)
    }
}
