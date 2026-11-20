//! login, logout, whoami.

use std::future::Future;
use std::io::Read;
use std::pin::Pin;
use std::time::Duration;

use hydronode_api::auth::{self, DeviceEvents, WebLoginEvents};
use hydronode_api::credentials::Credential;
use hydronode_api::{Error, Result};
use serde_json::{json, Value};

use crate::cli::LoginArgs;
use crate::ctx::{stdin_is_tty, Ctx};

struct Browser {
    no_browser: bool,
}

impl WebLoginEvents for Browser {
    fn open(&self, url: &str) -> bool {
        if self.no_browser {
            return false;
        }
        let ok = open::that_detached(url).is_ok();
        if ok {
            eprintln!("Opened your browser. Finish signing in there.");
        }
        ok
    }

    fn show_url(&self, url: &str) {
        eprintln!("If the browser does not open, visit:\n  {url}\n");
    }
}

struct Device {
    no_browser: bool,
    server: url::Url,
}

/// Only open a server-sent link that stays on the profile's server (same scheme, host, port).
fn same_origin(link: &str, server: &url::Url) -> bool {
    url::Url::parse(link).is_ok_and(|u| u.origin() == server.origin())
}

impl DeviceEvents for Device {
    fn show_code(&self, user_code: &str, uri: &str, _complete: Option<&str>) {
        eprintln!("Your one-time code: {user_code}");
        eprintln!("Open {uri} on any device, sign in and enter the code.");
        // Opens the plain page, never a link with the code in it: the server no longer sends one
        // (a prefilled code made device-code phishing a one-click affair), and typing the code is
        // the check that this terminal is yours.
        if !self.no_browser && same_origin(uri, &self.server) {
            let _ = open::that_detached(uri);
        }
        eprintln!("Waiting for confirmation …");
    }

    fn wait(&self, seconds: u64) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(tokio::time::sleep(Duration::from_secs(seconds)))
    }
}

enum Method {
    Web,
    Device,
    Token,
    WithToken,
}

pub async fn login(ctx: &Ctx, args: LoginArgs) -> Result<()> {
    let server = ctx.server()?;
    let method = if args.web {
        Method::Web
    } else if args.device {
        Method::Device
    } else if args.token {
        Method::Token
    } else if args.with_token {
        Method::WithToken
    } else if stdin_is_tty() {
        let choice = dialoguer::Select::new()
            .with_prompt(format!(
                "How do you want to sign in to {}?",
                server.host_str().unwrap_or("")
            ))
            .items(&[
                "Browser (recommended)",
                "Device code (SSH, no browser here)",
                "API token (hat_ or hrt_)",
            ])
            .default(0)
            .interact()
            .map_err(|e| Error::Other(e.to_string()))?;
        match choice {
            0 => Method::Web,
            1 => Method::Device,
            _ => Method::Token,
        }
    } else {
        return Err(Error::Usage(
            "choose how to sign in: --web, --device, --token or --with-token".into(),
        ));
    };

    let http = auth::http_client();
    let credential = match method {
        Method::Web => {
            let browser = Browser {
                no_browser: args.no_browser,
            };
            auth::login_web(&http, &server, &browser, Duration::from_secs(300)).await?
        }
        Method::Device => {
            let device = Device {
                no_browser: args.no_browser,
                server: server.clone(),
            };
            auth::login_device(&http, &server, &device).await?
        }
        Method::Token => {
            let token = rpassword::prompt_password("Paste your API token (input hidden): ")
                .map_err(|e| Error::Other(e.to_string()))?;
            auth::login_token(&http, &server, &token).await?.0
        }
        Method::WithToken => {
            let mut token = String::new();
            std::io::stdin().read_to_string(&mut token)?;
            auth::login_token(&http, &server, token.trim()).await?.0
        }
    };
    ctx.store().set(&ctx.profile, &credential)?;

    let me = ctx.client()?.get("/api/agent/v1/me", &[]).await.ok();
    let name = me
        .as_ref()
        .and_then(|m| m["account"]["name"].as_str())
        .map(str::to_string);
    if ctx.out.json {
        ctx.out.print_json(&json!({
            "profile": ctx.profile, "server": server.as_str().trim_end_matches('/'),
            "kind": credential.kind_name(), "readOnly": credential.is_read_only(), "name": name,
        }));
        return Ok(());
    }
    let who = name.map(|n| format!(" as {n}")).unwrap_or_default();
    ctx.out.line(&format!(
        "Signed in{who} on {} (profile {}).",
        server.host_str().unwrap_or(""),
        ctx.profile
    ));
    if credential.is_read_only() {
        ctx.out
            .line("This token only reads. Commands that change something need a full login.");
    }
    Ok(())
}

pub async fn logout(ctx: &Ctx) -> Result<()> {
    let client = ctx.client()?;
    let was_pat = matches!(ctx.store().get(&ctx.profile)?, Some(Credential::Pat { .. }));
    let had = client.logout().await?;
    if ctx.out.json {
        ctx.out
            .print_json(&json!({ "profile": ctx.profile, "loggedOut": had }));
    } else if !had {
        ctx.out
            .line(&format!("Not signed in (profile {}).", ctx.profile));
    } else if was_pat {
        ctx.out.line(&format!(
            "Forgot the API token. It still works until you revoke it ('{} tokens revoke').",
            ctx.bin
        ));
    } else {
        ctx.out.line("Signed out.");
    }
    Ok(())
}

pub async fn whoami(ctx: &Ctx) -> Result<()> {
    let client = ctx.client()?;
    let credential = client.credential().await?;
    let me = match client.get("/api/agent/v1/me", &[]).await {
        Ok(v) => v,
        Err(Error::Api { status: 404, .. }) => Value::Null,
        Err(e) => return Err(e),
    };
    let kind = me["auth"]["kind"]
        .as_str()
        .unwrap_or(credential.kind_name())
        .to_string();
    let read_only = credential.is_read_only() || kind == "read_pat";
    let server = client.base().as_str().trim_end_matches('/').to_string();
    let result = json!({
        "profile": ctx.profile,
        "server": server,
        "name": me["account"]["name"],
        "timeZone": me["account"]["timeZone"],
        "kind": kind,
        "scopes": me["auth"]["scopes"],
        "readOnly": read_only,
    });
    if ctx.out.json {
        ctx.out.print_json(&result);
        return Ok(());
    }
    let login = match kind.as_str() {
        "session" => "browser or device login".to_string(),
        "pat" => "API token (full)".to_string(),
        "read_pat" => "API token (read only)".to_string(),
        other => other.to_string(),
    };
    ctx.out.fields(&[
        ("Account", crate::output::s(&me["account"]["name"])),
        ("Server", server),
        ("Profile", ctx.profile.clone()),
        ("Login", login),
        ("Time zone", crate::output::s(&me["account"]["timeZone"])),
    ]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::same_origin;

    #[test]
    fn device_link_must_stay_on_the_server() {
        let server = url::Url::parse("https://hydronode.tech").unwrap();
        assert!(same_origin(
            "https://hydronode.tech/device?code=ABCD-EFGH",
            &server
        ));
        assert!(!same_origin("https://evil.example/device?code=x", &server));
        assert!(!same_origin("http://hydronode.tech/device", &server));
        assert!(!same_origin("javascript:alert(1)", &server));
        assert!(!same_origin("file:///etc/passwd", &server));
    }
}
