//! devices list|show|update (Fleet API v2: check, confirm, apply).

use hydronode_api::time::parse_duration;
use hydronode_api::{Error, Result};
use serde_json::{json, Value};

use super::seg;
use crate::cli::{DevicesArgs, DevicesCmd, DevicesUpdateArgs, PowerMode, VerifyMode};
use crate::ctx::{confirm, Ctx};
use crate::output::{s, time};

pub async fn run(ctx: &Ctx, args: DevicesArgs) -> Result<()> {
    let client = ctx.client()?;
    match args.cmd.unwrap_or(DevicesCmd::List) {
        DevicesCmd::List => {
            let v = client.get("/api/fleet/v2/devices", &[]).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let list = v["devices"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                ctx.out.line("No devices you manage.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list.iter().map(|d| row(ctx, d)).collect();
            ctx.out.table(
                &[
                    "NAME", "HEALTH", "BOARD", "FIRMWARE", "BATTERY", "SIGNAL", "JOB", "ID",
                ],
                &rows,
            );
        }
        DevicesCmd::Show { sensor } => {
            let v = client
                .get(&format!("/api/fleet/v2/devices/{}", seg(&sensor)?), &[])
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            show(ctx, &v);
        }
        DevicesCmd::Update(a) => update(ctx, &client, a).await?,
    }
    Ok(())
}

fn firmware(d: &Value) -> String {
    let f = &d["firmware"];
    if f.is_null() {
        return "-".into();
    }
    let mut t = match f["product"].as_str() {
        Some("HYDRONODE") | None => s(&f["version"]),
        Some("LIBRARY") => format!("own sketch {}", s(&f["version"])),
        Some(p) => format!("{} {}", p.to_lowercase(), s(&f["version"])),
    };
    if d["updateAvailable"].as_bool() == Some(true) {
        t.push_str(" ↑");
    }
    t
}

fn row(ctx: &Ctx, d: &Value) -> Vec<String> {
    let battery = d["battery"]["percent"]
        .as_f64()
        .map(|p| format!("{} %", p.round()))
        .unwrap_or_else(|| "-".into());
    let signal = match d["signal"]["rssi"].as_f64() {
        Some(r) => format!("{r} dBm {}", s(&d["signal"]["net"])),
        None => "-".into(),
    };
    let job = if d["job"].is_null() {
        "-".to_string()
    } else {
        format!(
            "{} {}",
            s(&d["job"]["changeLabel"]),
            s(&d["job"]["status"]).to_lowercase()
        )
    };
    vec![
        s(&d["name"]),
        ctx.out.status(&s(&d["health"])),
        s(&d["family"]),
        firmware(d),
        battery,
        signal,
        job,
        s(&d["id"]),
    ]
}

fn show(ctx: &Ctx, d: &Value) {
    let out = &ctx.out;
    out.line(&out.bold(&s(&d["name"])));
    let mut fields = vec![
        ("ID", s(&d["id"])),
        ("Health", out.status(&s(&d["health"]))),
        (
            "Board",
            format!("{} ({})", s(&d["family"]), s(&d["transport"])),
        ),
        ("Firmware", firmware(d)),
        ("Last seen", time(&d["lastSeenAt"])),
    ];
    if !d["config"].is_null() {
        fields.push((
            "Config",
            format!(
                "rev {}, {}, every {} s",
                s(&d["config"]["revision"]),
                s(&d["config"]["powerMode"]).to_lowercase(),
                s(&d["config"]["intervalSeconds"])
            ),
        ));
        if let Some(b) = d["config"]["blocked"].as_str() {
            fields.push(("Over the air", b.to_string()));
        }
    }
    if let Some(v) = d["verifyMode"]["effective"].as_str() {
        fields.push(("Verify", v.to_lowercase()));
    }
    out.fields(&fields);
    let jobs = d["jobs"].as_array().cloned().unwrap_or_default();
    if !jobs.is_empty() {
        out.line("");
        let rows: Vec<Vec<String>> = jobs
            .iter()
            .map(|j| {
                vec![
                    s(&j["changeLabel"]),
                    s(&j["title"]),
                    out.status(&s(&j["status"])),
                    s(&j["reason"]),
                    time(&j["createdAt"]),
                ]
            })
            .collect();
        out.table(&["CHANGE", "WHAT", "STATUS", "REASON", "CREATED"], &rows);
    }
}

fn power(p: PowerMode) -> &'static str {
    match p {
        PowerMode::AlwaysOn => "ALWAYS_ON",
        PowerMode::ModemSleep => "MODEM_SLEEP",
        PowerMode::LightSleep => "LIGHT_SLEEP",
        PowerMode::DeepSleep => "DEEP_SLEEP",
        PowerMode::Hibernate => "HIBERNATE",
    }
}

async fn update(
    ctx: &Ctx,
    client: &hydronode_api::client::Client,
    a: DevicesUpdateArgs,
) -> Result<()> {
    let mut ids = Vec::new();
    for id in &a.sensors {
        ids.push(seg(id)?);
    }
    let mut change = serde_json::Map::new();
    if let Some(f) = &a.firmware {
        let version = if f == "latest" {
            let fw = client.get("/api/fleet/v2/firmware", &[]).await?;
            fw["latest"]
                .as_str()
                .ok_or_else(|| Error::Other("the server reports no firmware release".into()))?
                .to_string()
        } else {
            f.clone()
        };
        change.insert("firmwareVersion".into(), json!(version));
    }
    if let Some(i) = &a.interval {
        let secs = parse_duration(i)?;
        if !(10..=604_800).contains(&secs) {
            return Err(Error::Usage("--interval must be between 10s and 7d".into()));
        }
        change.insert("intervalSeconds".into(), json!(secs));
    }
    if let Some(p) = a.power {
        change.insert("powerMode".into(), json!(power(p)));
    }
    if a.add_rssi {
        change.insert("addValues".into(), json!(["RSSI"]));
    }
    if change.is_empty() {
        return Err(Error::Usage(
            "nothing to change: give --firmware, --interval, --power or --add-rssi".into(),
        ));
    }
    let verify = a.verify.map(|v| match v {
        VerifyMode::Strict => "STRICT",
        VerifyMode::Ingest => "INGEST",
    });
    let request = json!({ "sensorIds": ids, "verifyMode": verify, "change": change });
    let check = client.post("/api/fleet/v2/changes/check", &request).await?;

    if !ctx.out.json {
        print_check(ctx, &check);
    }
    let counts = &check["counts"];
    let possible = counts["ok"].as_u64().unwrap_or(0) + counts["warn"].as_u64().unwrap_or(0);
    if possible == 0 {
        if ctx.out.json {
            ctx.out
                .print_json(&json!({ "check": check, "applied": null }));
        }
        return Err(Error::Other("no device can take this change".into()));
    }
    let low_battery = check["rows"]
        .as_array()
        .map(|r| r.iter().any(|x| x["lowBattery"].as_bool() == Some(true)))
        .unwrap_or(false);
    if low_battery && !a.confirm_low_battery {
        return Err(Error::Usage(
            "a device is below 30 % battery. Add --confirm-low-battery to update it anyway".into(),
        ));
    }
    let question = format!(
        "Start the update on {possible} device{}?",
        if possible == 1 { "" } else { "s" }
    );
    if !confirm(&question, a.yes)? {
        ctx.out.line("Nothing started.");
        return Ok(());
    }
    let mut apply = request.clone();
    apply["confirmLowBattery"] = json!(a.confirm_low_battery);
    let applied = client.post("/api/fleet/v2/changes/apply", &apply).await?;
    if ctx.out.json {
        ctx.out
            .print_json(&json!({ "check": check, "applied": applied }));
        return Ok(());
    }
    let jobs = applied["jobs"].as_array().map(|j| j.len()).unwrap_or(0);
    ctx.out.line(&format!(
        "Started {} with {jobs} job{}. Follow it with '{} devices'.",
        s(&applied["label"]),
        if jobs == 1 { "" } else { "s" },
        ctx.hn()
    ));
    Ok(())
}

fn print_check(ctx: &Ctx, check: &Value) {
    let rows: Vec<Vec<String>> = check["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let cells: Vec<String> = r["cells"]
                        .as_array()
                        .map(|c| {
                            c.iter()
                                .map(|cell| match cell["type"].as_str() {
                                    Some("CHANGE") => format!(
                                        "{} {} → {}",
                                        s(&cell["label"]).to_lowercase(),
                                        s(&cell["from"]),
                                        s(&cell["to"])
                                    ),
                                    Some("SAME") => {
                                        format!("{} unchanged", s(&cell["label"]).to_lowercase())
                                    }
                                    _ => format!(
                                        "{}: {}",
                                        s(&cell["label"]).to_lowercase(),
                                        s(&cell["note"])
                                    ),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let reasons: Vec<String> = r["reasons"]
                        .as_array()
                        .map(|x| x.iter().map(s).collect())
                        .unwrap_or_default();
                    vec![
                        s(&r["name"]),
                        ctx.out.status(&s(&r["verdict"])),
                        cells.join(", "),
                        reasons.join("; "),
                    ]
                })
                .collect()
        })
        .unwrap_or_default();
    ctx.out
        .table(&["DEVICE", "CHECK", "CHANGES", "NOTES"], &rows);
}
