//! status, set, command, console, anomalies, messages, stations, automations.

use std::time::Duration;

use hydronode_api::time::parse_instant_now;
use hydronode_api::{Error, Result};
use serde_json::{json, Value};

use super::{items, more_line, parse_value, resolve_point, seg};
use crate::cli::{
    AnomaliesArgs, AnomaliesCmd, AutomationsArgs, AutomationsCmd, CommandArgs, CommandType,
    ConsoleArgs, MessagesArgs, MessagesCmd, SetArgs, StationsArgs,
};
use crate::ctx::Ctx;
use crate::output::{s, time, with_unit};

pub async fn status(ctx: &Ctx) -> Result<()> {
    let v = ctx.client()?.get("/api/agent/v1/overview", &[]).await?;
    if ctx.out.json {
        ctx.out.print_json(&v);
        return Ok(());
    }
    let out = &ctx.out;
    let d = &v["devices"];
    let mut parts = vec![format!("{} devices", s(&d["total"]))];
    for (key, word) in [
        ("healthy", "healthy"),
        ("delayed", "delayed"),
        ("offline", "offline"),
        ("standby", "standby"),
        ("noData", "no data"),
    ] {
        if d[key].as_u64().unwrap_or(0) > 0 {
            parts.push(format!("{} {}", s(&d[key]), out.status(word)));
        }
    }
    out.fields(&[
        ("Devices", parts.join(", ")),
        ("Anomalies", format!("{} open", s(&v["openAnomalies"]))),
        ("Messages", format!("{} unread", s(&v["unreadMessages"]))),
    ]);
    let projects = v["projects"].as_array().cloned().unwrap_or_default();
    if !projects.is_empty() {
        out.line("");
        let rows: Vec<Vec<String>> = projects
            .iter()
            .flat_map(|p| {
                let pools = p["pools"].as_array().cloned().unwrap_or_default();
                if pools.is_empty() {
                    vec![vec![s(&p["name"]), "-".into(), "0".into()]]
                } else {
                    pools
                        .iter()
                        .map(|pool| vec![s(&p["name"]), s(&pool["name"]), s(&pool["sensors"])])
                        .collect()
                }
            })
            .collect();
        out.table(&["PROJECT", "POOL", "SENSORS"], &rows);
    }
    let attention = v["needsAttention"].as_array().cloned().unwrap_or_default();
    if !attention.is_empty() {
        out.line("");
        out.line(&out.bold("Needs attention"));
        let rows: Vec<Vec<String>> = attention
            .iter()
            .map(|a| {
                vec![
                    out.status(&s(&a["kind"])),
                    s(&a["sensor"]["name"]),
                    s(&a["detail"]),
                    s(&a["sensor"]["id"]),
                ]
            })
            .collect();
        out.table(&["KIND", "SENSOR", "DETAIL", "ID"], &rows);
    }
    Ok(())
}

pub async fn set(ctx: &Ctx, args: SetArgs) -> Result<()> {
    let client = ctx.client()?;
    // read-only tokens stop here, before any lookup
    if client.credential().await?.is_read_only() {
        return Err(Error::NeedsFullLogin(
            "a read-only token (hrt_) can only read".into(),
        ));
    }
    let point = resolve_point(&client, &args.target).await?;
    let v = client
        .post(
            &format!("/api/agent/v1/points/{point}/value"),
            &json!({ "value": parse_value(&args.value), "waitSeconds": args.wait }),
        )
        .await?;
    if ctx.out.json {
        ctx.out.print_json(&v);
        return Ok(());
    }
    let state = s(&v["state"]);
    let mut line = format!("{}: desired {}", ctx.out.status(&state), s(&v["desired"]));
    if !v["actual"].is_null() {
        line.push_str(&format!(", device reports {}", s(&v["actual"])));
    }
    if !v["reason"].is_null() {
        line.push_str(&format!(" ({})", s(&v["reason"])));
    }
    ctx.out.line(&line);
    if state == "FAILED" {
        return Err(Error::Other("the device did not take the value".into()));
    }
    Ok(())
}

fn command_value(raw: &str, t: Option<CommandType>) -> Result<(String, Value)> {
    let bad = |what: &str| Error::Usage(format!("'{raw}' is not a valid {what}"));
    let t = t.unwrap_or_else(|| match parse_value(raw) {
        Value::Bool(_) => CommandType::Bool,
        Value::Number(n) if n.is_i64() => {
            if i32::try_from(n.as_i64().unwrap_or(0)).is_ok() {
                CommandType::Int32
            } else {
                CommandType::Int64
            }
        }
        Value::Number(n) if n.is_u64() => CommandType::Uint64,
        _ => CommandType::String,
    });
    Ok(match t {
        CommandType::Bool => match parse_value(raw) {
            Value::Bool(b) => ("BOOL".into(), Value::Bool(b)),
            _ => return Err(bad("true/false value")),
        },
        CommandType::Int32 => (
            "INT32".into(),
            Value::from(raw.parse::<i32>().map_err(|_| bad("INT32"))?),
        ),
        CommandType::Uint32 => (
            "UINT32".into(),
            Value::from(raw.parse::<u32>().map_err(|_| bad("UINT32"))?),
        ),
        CommandType::Int64 => (
            "INT64".into(),
            Value::from(raw.parse::<i64>().map_err(|_| bad("INT64"))?),
        ),
        CommandType::Uint64 => (
            "UINT64".into(),
            Value::from(raw.parse::<u64>().map_err(|_| bad("UINT64"))?),
        ),
        CommandType::String => ("STRING".into(), Value::String(raw.to_string())),
    })
}

pub async fn command(ctx: &Ctx, args: CommandArgs) -> Result<()> {
    let (t, value) = command_value(&args.value, args.value_type)?;
    let v = ctx
        .client()?
        .post(
            &format!("/api/sensors/{}/commands", seg(&args.sensor)?),
            &json!({ "command": args.name, "type": t, "value": value }),
        )
        .await?;
    if ctx.out.json {
        ctx.out.print_json(&v);
    } else {
        ctx.out.line(&format!(
            "Queued {} = {} ({t}). The device gets it with its next delivery.",
            args.name,
            s(&value)
        ));
    }
    Ok(())
}

fn console_rows(
    ctx: &Ctx,
    events: &[Value],
    rejected_only: bool,
    with_sensor: bool,
) -> Vec<Vec<String>> {
    events
        .iter()
        .rev()
        .filter(|e| !rejected_only || e["outcome"].as_str() == Some("REJECTED"))
        .map(|e| {
            let mut row = vec![time(&e["at"])];
            if with_sensor {
                row.push(s(&e["sensorName"]));
            }
            row.push(s(&e["source"]));
            row.push(ctx.out.status(&s(&e["outcome"])));
            row.push(s(&e["status"]));
            row.push(s(&e["message"]));
            row
        })
        .collect()
}

pub async fn console(ctx: &Ctx, args: ConsoleArgs) -> Result<()> {
    let client = ctx.client()?;
    let credential = client.credential().await?;
    if !credential.is_session() {
        if args.follow {
            return Err(Error::NeedsFullLogin(
                "following the console needs a browser or device login".into(),
            ));
        }
        // tokens read the compact agent console
        let mut query = vec![("limit", args.limit.min(50).to_string())];
        if let Some(sensor) = &args.sensor {
            query.push(("sensor", seg(sensor)?));
        }
        if args.rejected {
            query.push(("rejected", "true".into()));
        }
        let v = client.get("/api/agent/v1/console", &query).await?;
        if ctx.out.json {
            ctx.out.print_json(&v);
            return Ok(());
        }
        let rows: Vec<Vec<String>> = items(&v)
            .iter()
            .map(|e| {
                vec![
                    s(&e["ago"]),
                    s(&e["sensor"]["name"]),
                    s(&e["transport"]),
                    if e["accepted"].as_bool() == Some(false) {
                        ctx.out.status("REJECTED")
                    } else {
                        ctx.out.status("ACCEPTED")
                    },
                    s(&e["reason"]),
                    s(&e["summary"]),
                ]
            })
            .collect();
        ctx.out.table(
            &["AGO", "SENSOR", "VIA", "OUTCOME", "REASON", "SUMMARY"],
            &rows,
        );
        return Ok(());
    }

    let path = match &args.sensor {
        Some(sensor) => format!("/api/sensors/{}/console/events", seg(sensor)?),
        None => "/api/console/v1/events".to_string(),
    };
    let with_sensor = args.sensor.is_none();
    let mut headers = vec!["AT"];
    if with_sensor {
        headers.push("SENSOR");
    }
    headers.extend(["SOURCE", "OUTCOME", "STATUS", "MESSAGE"]);

    let v = client
        .get(&path, &[("limit", args.limit.to_string())])
        .await?;
    let events = v["events"].as_array().cloned().unwrap_or_default();
    if ctx.out.json && !args.follow {
        ctx.out.print_json(&v);
        return Ok(());
    }
    let print = |events: &[Value], header: bool| {
        if ctx.out.json {
            for e in events.iter().rev() {
                if !args.rejected || e["outcome"].as_str() == Some("REJECTED") {
                    ctx.out.line(&e.to_string());
                }
            }
            return;
        }
        let rows = console_rows(ctx, events, args.rejected, with_sensor);
        if rows.is_empty() {
            return;
        }
        let text = crate::output::render_table(&headers, &rows, ctx.out.color);
        if header {
            crate::output::print_safe(&text);
        } else {
            // follow mode: no repeated header
            let body: String = text.lines().skip(1).map(|l| format!("{l}\n")).collect();
            crate::output::print_safe(&body);
        }
    };
    if events.is_empty() && !args.follow {
        ctx.out.line("No console lines in the last 48 hours.");
        return Ok(());
    }
    print(&events, true);
    if !args.follow {
        return Ok(());
    }
    let mut cursor = v["cursor"].clone();
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut query = vec![("limit", "200".to_string())];
        if !cursor.is_null() {
            query.push(("after", s(&cursor)));
        }
        let next = client.get(&path, &query).await?;
        let events = next["events"].as_array().cloned().unwrap_or_default();
        if !next["cursor"].is_null() {
            cursor = next["cursor"].clone();
        }
        print(&events, false);
    }
}

pub async fn anomalies(ctx: &Ctx, args: AnomaliesArgs) -> Result<()> {
    let client = ctx.client()?;
    if let Some(AnomaliesCmd::Show { id }) = args.cmd {
        let v = client
            .get(&format!("/api/agent/v1/anomalies/{}", seg(&id)?), &[])
            .await?;
        if ctx.out.json {
            ctx.out.print_json(&v);
            return Ok(());
        }
        let out = &ctx.out;
        out.line(&out.bold(&s(&v["title"])));
        let mut fields = vec![
            ("Sensor", s(&v["sensor"]["name"])),
            ("Severity", out.status(&s(&v["severity"]))),
            ("State", s(&v["state"])),
            ("Started", time(&v["startedAt"])),
            ("Last seen", time(&v["lastSeenAt"])),
        ];
        if !v["values"].is_null() {
            fields.push((
                "Values",
                format!(
                    "expected {}, observed {}",
                    with_unit(&v["values"]["expected"], &v["values"]["unit"]),
                    with_unit(&v["values"]["observed"], &v["values"]["unit"])
                ),
            ));
        }
        out.fields(&fields);
        if let Some(text) = v["explanation"].as_str() {
            out.line("");
            out.line(text);
        }
        return Ok(());
    }
    let mut query = vec![("state", if args.all { "all" } else { "open" }.to_string())];
    if let Some(sensor) = &args.sensor {
        query.push(("sensor", seg(sensor)?));
    }
    if let Some(since) = &args.since {
        query.push(("since", parse_instant_now(since)?.to_string()));
    }
    let v = client.get("/api/agent/v1/anomalies", &query).await?;
    if ctx.out.json {
        ctx.out.print_json(&v);
        return Ok(());
    }
    let list = items(&v);
    if list.is_empty() {
        ctx.out.line(if args.all {
            "No anomalies."
        } else {
            "No open anomalies."
        });
        return Ok(());
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|a| {
            vec![
                ctx.out.status(&s(&a["severity"])),
                s(&a["title"]),
                s(&a["sensor"]["name"]),
                s(&a["state"]),
                s(&a["ago"]),
                s(&a["id"]),
            ]
        })
        .collect();
    ctx.out.table(
        &["SEVERITY", "TITLE", "SENSOR", "STATE", "AGO", "ID"],
        &rows,
    );
    more_line(ctx, &v);
    Ok(())
}

pub async fn messages(ctx: &Ctx, args: MessagesArgs) -> Result<()> {
    let client = ctx.client()?;
    let (action, ids) = match args.cmd {
        None => {
            let query = if args.unread {
                vec![("unread", "true".to_string())]
            } else {
                vec![]
            };
            let v = client.get("/api/agent/v1/messages", &query).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let list = items(&v);
            if list.is_empty() {
                ctx.out.line(if args.unread {
                    "No unread messages."
                } else {
                    "No messages."
                });
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|m| {
                    vec![
                        if m["read"].as_bool() == Some(false) {
                            "●".into()
                        } else {
                            " ".into()
                        },
                        s(&m["category"]).to_lowercase(),
                        s(&m["title"]),
                        s(&m["ago"]),
                        s(&m["id"]),
                    ]
                })
                .collect();
            ctx.out.table(&["", "KIND", "TITLE", "AGO", "ID"], &rows);
            more_line(ctx, &v);
            return Ok(());
        }
        Some(MessagesCmd::Read { all: true, .. }) => {
            let v = client
                .post("/api/agent/v1/messages/read-all", &json!({}))
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
            } else {
                ctx.out
                    .line(&format!("Marked {} as read.", s(&v["marked"])));
            }
            return Ok(());
        }
        Some(MessagesCmd::Read { ids, .. }) => ("read", ids),
        Some(MessagesCmd::Unread { ids }) => ("unread", ids),
        Some(MessagesCmd::Archive { ids }) => ("archive", ids),
        Some(MessagesCmd::Restore { ids }) => ("restore", ids),
    };
    let mut done = Vec::new();
    for id in &ids {
        client
            .post(
                &format!("/api/agent/v1/messages/{}/{action}", seg(id)?),
                &json!({}),
            )
            .await?;
        done.push(id.clone());
    }
    if ctx.out.json {
        ctx.out
            .print_json(&json!({ "action": action, "ids": done }));
    } else {
        let word = match action {
            "read" => "Marked as read",
            "unread" => "Marked as unread",
            "archive" => "Archived",
            _ => "Restored",
        };
        ctx.out.line(&format!("{word}: {}.", done.len()));
    }
    Ok(())
}

pub async fn stations(ctx: &Ctx, args: StationsArgs) -> Result<()> {
    let mut query = Vec::new();
    if let Some(q) = &args.query {
        query.push(("q", q.clone()));
    }
    if let Some(near) = &args.near {
        let ok = near
            .split_once(',')
            .and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)))
            .filter(|(lat, lon)| lat.abs() <= 90.0 && lon.abs() <= 180.0);
        if ok.is_none() {
            return Err(Error::Usage(format!(
                "'{near}' is not LAT,LON like 52.52,13.40"
            )));
        }
        query.push(("near", near.replace(' ', "")));
        query.push(("radiusKm", args.radius.to_string()));
    }
    if query.is_empty() {
        return Err(Error::Usage(
            "search by name ('hn stations berlin') or place ('--near 52.52,13.40')".into(),
        ));
    }
    let v = ctx
        .client()?
        .get("/api/agent/v1/stations/public", &query)
        .await?;
    if ctx.out.json {
        ctx.out.print_json(&v);
        return Ok(());
    }
    let list = items(&v);
    if list.is_empty() {
        ctx.out.line("No public stations found.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|st| {
            let values: Vec<String> = st["values"]
                .as_array()
                .map(|vals| {
                    vals.iter()
                        .take(4)
                        .map(|x| {
                            format!(
                                "{} {}",
                                s(&x["name"]).to_lowercase(),
                                with_unit(&x["value"], &x["unit"])
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            vec![
                s(&st["name"]),
                st["distanceKm"]
                    .as_f64()
                    .map(|d| format!("{} km", crate::output::num((d * 10.0).round() / 10.0)))
                    .unwrap_or_else(|| "-".into()),
                values.join(", "),
                s(&st["id"]),
            ]
        })
        .collect();
    ctx.out.table(&["STATION", "DISTANCE", "NOW", "ID"], &rows);
    Ok(())
}

pub async fn automations(ctx: &Ctx, args: AutomationsArgs) -> Result<()> {
    let client = ctx.client()?;
    match args.cmd.unwrap_or(AutomationsCmd::List) {
        AutomationsCmd::List => {
            let v = client.get("/api/agent/v1/automations", &[]).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let list = items(&v);
            if list.is_empty() {
                ctx.out.line("No automations.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|a| {
                    vec![
                        if a["enabled"].as_bool() == Some(true) {
                            ctx.out.status("on")
                        } else {
                            "off".into()
                        },
                        s(&a["name"]),
                        s(&a["trigger"]),
                        s(&a["ago"]),
                        s(&a["id"]),
                    ]
                })
                .collect();
            ctx.out
                .table(&["", "NAME", "WHEN", "LAST RUN", "ID"], &rows);
            more_line(ctx, &v);
        }
        AutomationsCmd::Show { id } => {
            let v = client
                .get(&format!("/api/agent/v1/automations/{}", seg(&id)?), &[])
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let actions: Vec<String> = v["actions"]
                .as_array()
                .map(|a| a.iter().map(s).collect())
                .unwrap_or_default();
            ctx.out.line(&ctx.out.bold(&s(&v["name"])));
            ctx.out.fields(&[
                ("Enabled", s(&v["enabled"])),
                ("When", s(&v["trigger"])),
                ("Then", actions.join("; ")),
                ("Last run", time(&v["lastFired"])),
            ]);
        }
        cmd => {
            let (id, enabled) = match cmd {
                AutomationsCmd::Enable { id } => (id, true),
                AutomationsCmd::Disable { id } => (id, false),
                _ => unreachable!(),
            };
            let v = client
                .put(
                    &format!("/api/agent/v1/automations/{}/enabled", seg(&id)?),
                    &json!({ "enabled": enabled }),
                )
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
            } else {
                ctx.out.line(if enabled {
                    "Switched on."
                } else {
                    "Switched off."
                });
            }
        }
    }
    Ok(())
}
