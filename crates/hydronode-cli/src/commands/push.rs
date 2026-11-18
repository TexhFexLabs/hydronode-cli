//! push and simulate: the signed device protocol.

use std::io::Read;
use std::time::Duration;

use hydronode_api::client::Client;
use hydronode_api::device::{send_round, sign_round};
use hydronode_api::time::parse_duration;
use hydronode_api::{Error, Result};
use serde_json::{json, Value};

use super::seg;
use crate::cli::{PushArgs, SimulateArgs};
use crate::ctx::Ctx;

/// The sensor secret: env, stdin, or the owner's credentials endpoint. Never printed.
async fn secret(client: &Client, sensor: &str, from_stdin: bool) -> Result<String> {
    if from_stdin {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        let t = text.trim().to_string();
        if t.is_empty() {
            return Err(Error::Usage("no secret on stdin".into()));
        }
        return Ok(t);
    }
    if let Ok(t) = std::env::var("HYDRONODE_SENSOR_SECRET") {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_string());
        }
    }
    let v = client
        .get(&format!("/api/sensors/{sensor}/credentials"), &[])
        .await?;
    v["secret"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Other("the server sent no secret for this sensor".into()))
}

fn parse_pairs(pairs: &[String]) -> Result<Vec<(String, f64)>> {
    pairs
        .iter()
        .map(|p| {
            let (t, v) = p.split_once('=').ok_or_else(|| {
                Error::Usage(format!("'{p}' is not TYPE=VALUE like TEMPERATURE=21.4"))
            })?;
            let value: f64 = v
                .trim()
                .parse()
                .map_err(|_| Error::Usage(format!("'{v}' in '{p}' is not a number")))?;
            Ok((t.trim().to_ascii_uppercase(), value))
        })
        .collect()
}

fn answer_line(status: u16, body: &Value) -> String {
    let accepted = body["accepted"].as_u64().unwrap_or(0);
    let rejected = body["rejected"].as_array().map(|r| r.len()).unwrap_or(0);
    let commands = body["commands"].as_array().map(|r| r.len()).unwrap_or(0);
    let mut line = format!("{status}: {accepted} accepted");
    if rejected > 0 {
        line.push_str(&format!(", {rejected} rejected"));
    }
    if commands > 0 {
        line.push_str(&format!(
            ", {commands} command(s) waiting (not run by the CLI)"
        ));
    }
    line
}

fn status_error(status: u16, body: &Value) -> Error {
    let text = match body {
        Value::String(t) => t.clone(),
        other => other["message"]
            .as_str()
            .or(other["error"].as_str())
            .unwrap_or("")
            .to_string(),
    };
    Error::Api {
        status,
        code: body["error"].as_str().map(str::to_string),
        message: if text.is_empty() {
            format!("the server answered {status}")
        } else {
            text
        },
    }
}

pub async fn push(ctx: &Ctx, args: PushArgs) -> Result<()> {
    let values = parse_pairs(&args.values)?;
    let sensor = seg(&args.sensor)?;
    let client = ctx.client()?;
    let secret = secret(&client, &sensor, args.secret_stdin).await?;
    let now = jiff::Timestamp::now().as_second();
    let round = sign_round(&sensor, &secret, now, &values)?;
    let (status, body) = send_round(client.http(), client.base(), &sensor, &round).await?;
    if !(200..300).contains(&status) {
        return Err(status_error(status, &body));
    }
    if ctx.out.json {
        ctx.out
            .print_json(&json!({ "status": status, "answer": body }));
    } else {
        ctx.out.line(&answer_line(status, &body));
    }
    Ok(())
}

pub async fn simulate(ctx: &Ctx, args: SimulateArgs) -> Result<()> {
    let every = parse_duration(&args.every)?;
    if every < 10 {
        return Err(Error::Usage(
            "--every must be at least 10s (HydroNode limits how often a sensor may send)".into(),
        ));
    }
    let sensor = seg(&args.sensor)?;
    let client = ctx.client()?;
    let secret = secret(&client, &sensor, args.secret_stdin).await?;
    let profile = format!("{:?}", args.kind).to_lowercase();
    eprintln!(
        "Simulating a {profile} device on sensor {sensor}, every {every} s. This writes real data \
         into the sensor, so use a test sensor. Ctrl-C stops."
    );
    // tests run rounds without waiting
    let tick = if std::env::var_os("HYDRONODE_SIMULATE_FAST").is_some() {
        Duration::from_millis(every as u64)
    } else {
        Duration::from_secs(every as u64)
    };
    let mut rng = rand::rng();
    let mut sent = 0u64;
    let mut failures = 0u32;
    loop {
        let now = jiff::Timestamp::now().as_second();
        let values = crate::simulate::sample(args.kind, now, &mut rng);
        let round = sign_round(&sensor, &secret, now, &values)?;
        match send_round(client.http(), client.base(), &sensor, &round).await {
            Ok((status, body)) if (200..300).contains(&status) => {
                failures = 0;
                if ctx.out.json {
                    ctx.out.line(&json!({ "at": now, "values": values.iter().map(|(t, v)| json!({"type": t, "value": v})).collect::<Vec<_>>(), "status": status, "answer": body }).to_string());
                } else {
                    let text: Vec<String> = values
                        .iter()
                        .map(|(t, v)| format!("{}={}", t.to_lowercase(), crate::output::num(*v)))
                        .collect();
                    ctx.out.line(&format!(
                        "{}  {}  {}",
                        jiff::Timestamp::now().strftime("%H:%M:%S"),
                        text.join(" "),
                        answer_line(status, &body)
                    ));
                }
            }
            Ok((status, body)) => {
                let err = status_error(status, &body);
                if matches!(status, 401 | 403 | 404) {
                    return Err(err);
                }
                failures += 1;
                eprintln!("warning: {err} ({status})");
            }
            Err(e) => {
                failures += 1;
                eprintln!("warning: {e}");
            }
        }
        if failures >= 10 {
            return Err(Error::Other("10 failed rounds in a row, stopping".into()));
        }
        sent += 1;
        if args.count.is_some_and(|c| sent >= c) {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(tick) => {}
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Stopped after {sent} round(s).");
                return Ok(());
            }
        }
    }
}
