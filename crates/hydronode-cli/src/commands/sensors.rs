//! sensors list|show|create|rename|delete|credentials.

use hydronode_api::Result;
use serde_json::{json, Value};

use super::seg;
use crate::cli::{SensorsArgs, SensorsCmd};
use crate::ctx::{confirm, Ctx};
use crate::output::{s, time, with_unit};

pub async fn run(ctx: &Ctx, args: SensorsArgs) -> Result<()> {
    let client = ctx.client()?;
    match args.cmd.unwrap_or(SensorsCmd::List) {
        SensorsCmd::List => {
            let v = client.get("/api/sensors", &[]).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                ctx.out.line(&format!(
                    "No sensors yet. Create one with '{} sensors create <name>'.",
                    ctx.hn()
                ));
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list.iter().map(|x| list_row(ctx, x)).collect();
            ctx.out.table(
                &["ID", "NAME", "ACCESS", "STATUS", "LAST VALUE", "AT"],
                &rows,
            );
        }
        SensorsCmd::Show { sensor } => {
            let v = client
                .get(&format!("/api/agent/v1/sensors/{}", seg(&sensor)?), &[])
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            show(ctx, &v);
        }
        SensorsCmd::Create { name, outdoor } => {
            let mut v = client
                .post("/api/sensors", &json!({ "name": name, "outdoor": outdoor }))
                .await?;
            // the answer carries the secret once; it is not printed here
            if let Some(o) = v.as_object_mut() {
                o.remove("secret");
            }
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            let id = s(&v["sensorId"]);
            ctx.out.line(&format!("Created {} ({id}).", s(&v["name"])));
            ctx.out.line(&format!(
                "Device secret: '{} sensors credentials {id}'.",
                ctx.hn()
            ));
        }
        SensorsCmd::Rename { sensor, name } => {
            let v = client
                .patch(
                    &format!("/api/sensors/{}", seg(&sensor)?),
                    &json!({ "name": name }),
                )
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
            } else {
                ctx.out.line(&format!("Renamed to {name}."));
            }
        }
        SensorsCmd::Delete { sensor, yes } => {
            let id = seg(&sensor)?;
            if !confirm(
                &format!("Delete sensor {id} with all its readings? This cannot be undone."),
                yes,
            )? {
                ctx.out.line("Nothing deleted.");
                return Ok(());
            }
            let v = client.delete(&format!("/api/sensors/{id}"), &[]).await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
            } else {
                ctx.out.line("Deleted.");
            }
        }
        SensorsCmd::Credentials { sensor } => {
            let v = client
                .get(&format!("/api/sensors/{}/credentials", seg(&sensor)?), &[])
                .await?;
            if ctx.out.json {
                ctx.out.print_json(&v);
                return Ok(());
            }
            ctx.out.fields(&[
                ("Sensor ID", s(&v["sensorId"])),
                ("Secret", s(&v["secret"])),
            ]);
            eprintln!("Keep the secret private: it lets anyone send values as this sensor.");
        }
    }
    Ok(())
}

/// HEALTHY / DELAYED / OFFLINE / NO_DATA like the web: the server's status when it sends one
/// (standby), otherwise the age of the newest value against `monitoring.delayedAfterSeconds` /
/// `offlineAfterSeconds`; old answers without them use 3× / 12× the interval (floors 5 / 15 min).
pub(crate) fn sensor_status(x: &Value, now: jiff::Timestamp) -> &'static str {
    if let Some(s) = x["monitoring"]["status"].as_str() {
        return match s {
            "HEALTHY" => "HEALTHY",
            "DELAYED" => "DELAYED",
            "OFFLINE" => "OFFLINE",
            "STANDBY" => "STANDBY",
            "NO_DATA" => "NO_DATA",
            _ => "-",
        };
    }
    let newest = x["latestValues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["timestamp"].as_str()?.parse::<jiff::Timestamp>().ok())
        .max();
    let Some(newest) = newest else {
        return "NO_DATA";
    };
    let interval = x["monitoring"]["intervalSeconds"]
        .as_i64()
        .or(x["expectedReportIntervalSeconds"].as_i64());
    let delayed = x["monitoring"]["delayedAfterSeconds"]
        .as_i64()
        .or(interval.map(|i| (3 * i).max(300)));
    let offline = x["monitoring"]["offlineAfterSeconds"]
        .as_i64()
        .or(interval.map(|i| (12 * i).max(900)));
    let age = now.as_second() - newest.as_second();
    match (delayed, offline) {
        (_, Some(o)) if age > o => "OFFLINE",
        (Some(d), _) if age > d => "DELAYED",
        (None, None) => "-",
        _ => "HEALTHY",
    }
}

fn list_row(ctx: &Ctx, x: &Value) -> Vec<String> {
    let access = if x["isOwner"].as_bool() == Some(true) {
        "owner".to_string()
    } else {
        x["accessLevel"]
            .as_str()
            .map(|a| a.to_lowercase())
            .unwrap_or_else(|| "-".into())
    };
    let status = sensor_status(x, jiff::Timestamp::now());
    let latest = x["latestValues"]
        .as_array()
        .and_then(|vals| {
            vals.iter()
                .max_by(|a, b| a["timestamp"].as_str().cmp(&b["timestamp"].as_str()))
        })
        .cloned()
        .unwrap_or(Value::Null);
    let last = if latest.is_null() {
        "-".to_string()
    } else {
        format!(
            "{} {}",
            s(&latest["type"]).to_lowercase(),
            with_unit(&latest["value"], &latest["unit"])
        )
    };
    vec![
        s(&x["id"]),
        s(&x["name"]),
        access,
        ctx.out.status(status),
        last,
        time(&latest["timestamp"]),
    ]
}

fn show(ctx: &Ctx, v: &Value) {
    let out = &ctx.out;
    out.line(&out.bold(&s(&v["name"])));
    let mut fields = vec![
        ("ID", s(&v["id"])),
        (
            "Status",
            format!(
                "{} (last data {} ago)",
                out.status(&s(&v["status"])),
                s(&v["ago"])
            ),
        ),
    ];
    if !v["project"].is_null() {
        fields.push((
            "Project",
            format!("{} › {}", s(&v["project"]["name"]), s(&v["pool"]["name"])),
        ));
    }
    if !v["battery"].is_null() {
        fields.push((
            "Battery",
            format!(
                "{} % ({} V)",
                s(&v["battery"]["percent"]),
                s(&v["battery"]["voltage"])
            ),
        ));
    }
    if !v["signal"].is_null() {
        fields.push((
            "Signal",
            format!(
                "{} dBm {}",
                s(&v["signal"]["rssi"]),
                s(&v["signal"]["kind"])
            ),
        ));
    }
    if !v["firmware"].is_null() {
        fields.push(("Firmware", s(&v["firmware"])));
    }
    out.fields(&fields);
    let points = v["points"].as_array().cloned().unwrap_or_default();
    if points.is_empty() {
        return;
    }
    out.line("");
    let rows: Vec<Vec<String>> = points
        .iter()
        .map(|p| {
            vec![
                s(&p["name"]),
                s(&p["type"]),
                with_unit(&p["value"], &p["unit"]),
                s(&p["ago"]),
                if p["writable"].as_bool() == Some(true) {
                    "yes".into()
                } else {
                    "-".into()
                },
                s(&p["id"]),
            ]
        })
        .collect();
    out.table(&["POINT", "TYPE", "VALUE", "AGE", "WRITABLE", "ID"], &rows);
    let id = s(&v["id"]);
    let example = points
        .iter()
        .find_map(|p| p["type"].as_str())
        .unwrap_or("temperature")
        .to_lowercase();
    out.line("");
    out.line(&out.dim(&format!(
        "History: {bin} values {id}/{example} --from 7d. Writable points: {bin} set {id}/<type> <value>.",
        bin = ctx.hn()
    )));
}

#[cfg(test)]
mod status_tests {
    use super::sensor_status;
    use serde_json::json;

    const NOW: &str = "2026-11-17T22:00:00Z";

    fn sensor(ts: Option<&str>, monitoring: serde_json::Value) -> serde_json::Value {
        let vals = ts.map(|t| json!([{ "timestamp": t, "type": "TEMPERATURE", "value": 21.0 }]));
        json!({ "monitoring": monitoring, "latestValues": vals.unwrap_or(json!([])) })
    }

    fn now() -> jiff::Timestamp {
        NOW.parse().unwrap()
    }

    #[test]
    fn status_comes_from_the_age_of_the_newest_value_and_the_thresholds() {
        let m = json!({ "intervalSeconds": 600, "delayedAfterSeconds": 1800, "offlineAfterSeconds": 7200 });
        assert_eq!(
            sensor_status(&sensor(Some("2026-11-17T21:55:00Z"), m.clone()), now()),
            "HEALTHY"
        );
        assert_eq!(
            sensor_status(&sensor(Some("2026-11-17T21:20:00Z"), m.clone()), now()),
            "DELAYED"
        );
        assert_eq!(
            sensor_status(&sensor(Some("2026-11-17T19:00:00Z"), m.clone()), now()),
            "OFFLINE"
        );
        assert_eq!(sensor_status(&sensor(None, m), now()), "NO_DATA");
    }

    #[test]
    fn a_status_from_the_server_wins() {
        let m = json!({ "status": "STANDBY", "delayedAfterSeconds": 1800, "offlineAfterSeconds": 7200 });
        assert_eq!(
            sensor_status(&sensor(Some("2026-11-17T19:00:00Z"), m), now()),
            "STANDBY"
        );
    }

    #[test]
    fn old_answers_without_thresholds_use_three_and_twelve_intervals() {
        let mut s = sensor(Some("2026-11-17T21:00:00Z"), json!(null));
        s["expectedReportIntervalSeconds"] = json!(600);
        assert_eq!(sensor_status(&s, now()), "DELAYED");
        s["expectedReportIntervalSeconds"] = json!(60);
        assert_eq!(sensor_status(&s, now()), "OFFLINE");
    }
}
