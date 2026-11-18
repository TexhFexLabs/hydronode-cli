//! api, config, profile, completion, open.

use std::io::Read;

use hydronode_api::config::{check_server, Profile, DEFAULT_PROFILE};
use hydronode_api::{Error, Result};
use reqwest::Method;
use serde_json::{json, Value};

use super::seg;
use crate::cli::{
    ApiArgs, CompletionArgs, ConfigArgs, ConfigCmd, OpenArgs, ProfileArgs, ProfileCmd,
};
use crate::ctx::Ctx;

pub async fn api(ctx: &Ctx, args: ApiArgs) -> Result<()> {
    let (method, path) = match args.args.as_slice() {
        [path] => (Method::GET, path.clone()),
        [m, path] => (
            Method::from_bytes(m.to_ascii_uppercase().as_bytes())
                .map_err(|_| Error::Usage(format!("'{m}' is not an HTTP method")))?,
            path.clone(),
        ),
        _ => unreachable!("clap allows 1 or 2"),
    };
    if !path.starts_with('/') {
        return Err(Error::Usage(format!(
            "'{path}' must be a path on the profile's server, like /api/sensors"
        )));
    }
    let mut query: Vec<(&str, String)> = Vec::new();
    let pairs: Vec<(String, String)> = args
        .query
        .iter()
        .map(|q| {
            q.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| Error::Usage(format!("'{q}' is not KEY=VALUE")))
        })
        .collect::<Result<_>>()?;
    for (k, v) in &pairs {
        query.push((k.as_str(), v.clone()));
    }
    let body = match &args.data {
        None => None,
        Some(d) => {
            let text = if d == "-" {
                let mut t = String::new();
                std::io::stdin().read_to_string(&mut t)?;
                t
            } else if let Some(file) = d.strip_prefix('@') {
                std::fs::read_to_string(file)?
            } else {
                d.clone()
            };
            Some(
                serde_json::from_str::<Value>(&text)
                    .map_err(|e| Error::Usage(format!("the body is not JSON: {e}")))?,
            )
        }
    };
    let client = ctx.client()?;
    let raw = client.request(method, &path, &query, body.as_ref()).await?;
    let is_json = raw
        .content_type
        .as_deref()
        .is_some_and(|c| c.contains("json"));
    if is_json || raw.body.is_empty() {
        if !raw.body.is_empty() {
            ctx.out.print_json(&raw.json());
        }
    } else {
        crate::output::print_safe(&String::from_utf8_lossy(&raw.body));
    }
    if !(200..300).contains(&raw.status) {
        let e = hydronode_api::client::api_error(raw.status, &String::from_utf8_lossy(&raw.body));
        return Err(e);
    }
    Ok(())
}

pub fn config(ctx: &mut Ctx, args: ConfigArgs) -> Result<()> {
    let key_error =
        |k: &str| Error::Usage(format!("unknown setting '{k}', known: default-profile"));
    match args.cmd.unwrap_or(ConfigCmd::List) {
        ConfigCmd::List => {
            let default = ctx
                .config
                .default_profile
                .clone()
                .unwrap_or_else(|| DEFAULT_PROFILE.into());
            if ctx.out.json {
                ctx.out.print_json(&json!({ "default-profile": default }));
            } else {
                ctx.out.fields(&[("default-profile", default)]);
            }
        }
        ConfigCmd::Get { key } => match key.as_str() {
            "default-profile" => ctx.out.line(
                &ctx.config
                    .default_profile
                    .clone()
                    .unwrap_or_else(|| DEFAULT_PROFILE.into()),
            ),
            other => return Err(key_error(other)),
        },
        ConfigCmd::Set { key, value } => match key.as_str() {
            "default-profile" => {
                ctx.config.profile(&value)?;
                ctx.config.default_profile = Some(value.clone());
                ctx.save_config()?;
                ctx.out.line(&format!("default-profile = {value}"));
            }
            other => return Err(key_error(other)),
        },
        ConfigCmd::Path => ctx.out.line(&ctx.config_path().display().to_string()),
    }
    Ok(())
}

fn valid_profile_name(name: &str) -> Result<()> {
    if !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(Error::Usage(format!(
            "'{name}' is not a profile name, use letters, digits, - and _"
        )))
    }
}

pub fn profile(ctx: &mut Ctx, args: ProfileArgs) -> Result<()> {
    match args.cmd.unwrap_or(ProfileCmd::List) {
        ProfileCmd::List => {
            let all = ctx.config.all_profiles();
            if ctx.out.json {
                ctx.out
                    .print_json(&json!({ "active": ctx.profile, "profiles": all }));
                return Ok(());
            }
            let rows: Vec<Vec<String>> = all
                .iter()
                .map(|(name, p)| {
                    vec![
                        if *name == ctx.profile {
                            "*".into()
                        } else {
                            " ".into()
                        },
                        name.clone(),
                        p.server.clone(),
                    ]
                })
                .collect();
            ctx.out.table(&["", "PROFILE", "SERVER"], &rows);
        }
        ProfileCmd::Add { name, server } => {
            valid_profile_name(&name)?;
            let url = check_server(&server)?;
            let server = url.as_str().trim_end_matches('/').to_string();
            // A login belongs to one server. Moving a profile elsewhere must never send its
            // tokens to the new host, so the stored login goes with the old server.
            let moved = ctx
                .config
                .profile(&name)
                .ok()
                .is_some_and(|p| p.server.trim_end_matches('/') != server);
            ctx.config.profiles.insert(
                name.clone(),
                Profile {
                    server: server.clone(),
                },
            );
            ctx.save_config()?;
            if moved {
                ctx.store().delete(&name)?;
                ctx.out.line(&format!(
                    "Profile {name} → {server}, signed out of the old server. Sign in with 'hn -p {name} login'."
                ));
            } else {
                ctx.out.line(&format!(
                    "Profile {name} → {server}. Sign in with 'hn -p {name} login'."
                ));
            }
        }
        ProfileCmd::Remove { name } => {
            if ctx.config.profiles.remove(&name).is_none() {
                return Err(Error::Usage(format!("no profile '{name}'")));
            }
            if ctx.config.default_profile.as_deref() == Some(name.as_str()) {
                ctx.config.default_profile = None;
            }
            ctx.save_config()?;
            ctx.store().delete(&name)?;
            ctx.out
                .line(&format!("Removed profile {name} and its stored login."));
        }
        ProfileCmd::Use { name } => {
            ctx.config.profile(&name)?;
            ctx.config.default_profile = Some(name.clone());
            ctx.save_config()?;
            ctx.out.line(&format!("Using profile {name} by default."));
        }
    }
    Ok(())
}

pub fn completion(ctx: &Ctx, args: CompletionArgs) -> Result<()> {
    let mut cmd = crate::cli::command(&ctx.bin);
    let mut buf = Vec::new();
    clap_complete::generate(args.shell, &mut cmd, ctx.bin.clone(), &mut buf);
    crate::output::print_safe(&String::from_utf8_lossy(&buf));
    Ok(())
}

pub fn open(ctx: &Ctx, args: OpenArgs) -> Result<()> {
    let base = ctx.server()?;
    let base = base.as_str().trim_end_matches('/');
    let path = match args.what.as_deref() {
        None | Some("dashboard") | Some("devices") => "/dashboard".to_string(),
        Some("messages") => "/messages".into(),
        Some("stations") => "/stations".into(),
        Some("docs") => "/docs/guide/cli".into(),
        Some(id) => format!("/sensors/{}", seg(id)?),
    };
    let url = format!("{base}{path}");
    if args.print || std::env::var_os("HYDRONODE_NO_BROWSER").is_some() {
        ctx.out.line(&url);
        return Ok(());
    }
    if open::that_detached(&url).is_err() {
        ctx.out.line(&url);
    }
    Ok(())
}
