//! tokens and sessions.

use hydronode_api::Result;
use serde_json::json;

use super::{items, seg};
use crate::cli::{SessionsArgs, SessionsCmd, TokensArgs, TokensCmd};
use crate::ctx::{confirm, Ctx};
use crate::output::{s, time};

pub async fn tokens(ctx: &Ctx, args: TokensArgs) -> Result<()> {
    let client = ctx.client()?;
    match args.cmd.unwrap_or(TokensCmd::List) {
        TokensCmd::List => {
            let v = client.get("/api/user/api-tokens", &[]).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let list = items(&v);
            if list.is_empty() {
                ctx.out.line("No API tokens.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|t| {
                    vec![
                        s(&t["name"]),
                        if s(&t["scope"]) == "READ" {
                            "read only".into()
                        } else {
                            "full".into()
                        },
                        format!("{}…", s(&t["tokenPrefix"])),
                        time(&t["createdAt"]),
                        time(&t["lastUsedAt"]),
                        s(&t["id"]),
                    ]
                })
                .collect();
            ctx.out.table(
                &["NAME", "ACCESS", "TOKEN", "CREATED", "LAST USED", "ID"],
                &rows,
            );
        }
        TokensCmd::Create { name, read } => {
            let v = client
                .post(
                    "/api/user/api-tokens",
                    &json!({ "name": name, "scope": if read { "READ" } else { "FULL" } }),
                )
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            // the one place a new token is shown: the user asked for it, it is never shown again
            ctx.out.line(&s(&v["token"]));
            eprintln!(
                "Copy it now, it is not shown again. Revoke it with '{} tokens revoke {}'.",
                ctx.hn(),
                s(&v["id"])
            );
        }
        TokensCmd::Revoke { id, yes } => {
            let id = seg(&id)?;
            if !confirm(
                &format!("Revoke token {id}? Anything using it stops working."),
                yes,
            )? {
                ctx.out.line("Nothing revoked.");
                return Ok(());
            }
            client
                .delete(&format!("/api/user/api-tokens/{id}"), &[])
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&json!({ "revoked": id }));
            } else {
                ctx.out.line("Revoked.");
            }
        }
    }
    Ok(())
}

fn client_name(c: &str) -> String {
    match c {
        "cli" => "HydroNode CLI".into(),
        "web" => "Web".into(),
        "ios" => "iOS app".into(),
        "android" => "Android app".into(),
        other => other.to_string(),
    }
}

pub async fn sessions(ctx: &Ctx, args: SessionsArgs) -> Result<()> {
    let client = ctx.client()?;
    match args.cmd.unwrap_or(SessionsCmd::List) {
        SessionsCmd::List => {
            let v = client.get("/api/user/sessions/v1", &[]).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let rows: Vec<Vec<String>> = items(&v)
                .iter()
                .map(|x| {
                    let mut agent = s(&x["userAgent"]);
                    if agent.chars().count() > 40 {
                        agent = format!("{}…", agent.chars().take(40).collect::<String>());
                    }
                    vec![
                        if x["current"].as_bool() == Some(true) {
                            "*".into()
                        } else {
                            " ".into()
                        },
                        client_name(x["client"].as_str().unwrap_or("-")),
                        agent,
                        time(&x["lastActiveAt"]),
                        s(&x["id"]),
                    ]
                })
                .collect();
            ctx.out
                .table(&["", "CLIENT", "DEVICE", "LAST ACTIVE", "ID"], &rows);
        }
        SessionsCmd::Revoke { id, others, yes } => {
            if others {
                if !confirm("Sign out every other session?", yes)? {
                    ctx.out.line("Nothing changed.");
                    return Ok(());
                }
                let v = client
                    .delete("/api/user/sessions/v1", &[("others", "true".into())])
                    .await?;
                if ctx.out.json {
                    ctx.out.print_json(&v);
                } else {
                    ctx.out
                        .line(&format!("Signed out {} session(s).", s(&v["revoked"])));
                }
            } else {
                let id = seg(id.as_deref().unwrap_or_default())?;
                if !confirm(&format!("Sign out session {id}?"), yes)? {
                    ctx.out.line("Nothing changed.");
                    return Ok(());
                }
                client
                    .delete(&format!("/api/user/sessions/v1/{id}"), &[])
                    .await?;
                if ctx.out.json {
                    ctx.out.print_json(&json!({ "revoked": id }));
                } else {
                    ctx.out.line("Signed out.");
                }
            }
        }
    }
    Ok(())
}
