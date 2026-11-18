//! One module per command group. Shared helpers below.

mod account;
mod auth;
mod data;
mod devices;
mod export;
mod misc;
mod push;
mod sensors;
mod values;

use hydronode_api::client::Client;
use hydronode_api::time::parse_instant_now;
use hydronode_api::{Error, Result};
use serde_json::Value;

use crate::cli::{Cli, Command, TimeRange};
use crate::ctx::Ctx;

pub async fn dispatch(cli: Cli, bin: &str) -> Result<()> {
    let mut ctx = Ctx::load(cli.profile.as_deref(), cli.json, bin)?;
    match cli.command {
        Command::Login(a) => auth::login(&ctx, a).await,
        Command::Logout => auth::logout(&ctx).await,
        Command::Whoami => auth::whoami(&ctx).await,
        Command::Status => data::status(&ctx).await,
        Command::Sensors(a) => sensors::run(&ctx, a).await,
        Command::Values(a) => values::run(&ctx, a).await,
        Command::Watch(a) => crate::watch::run(&ctx, a).await,
        Command::Set(a) => data::set(&ctx, a).await,
        Command::Command(a) => data::command(&ctx, a).await,
        Command::Export(a) => export::run(&ctx, a).await,
        Command::Devices(a) => devices::run(&ctx, a).await,
        Command::Console(a) => data::console(&ctx, a).await,
        Command::Push(a) => push::push(&ctx, a).await,
        Command::Simulate(a) => push::simulate(&ctx, a).await,
        Command::Anomalies(a) => data::anomalies(&ctx, a).await,
        Command::Messages(a) => data::messages(&ctx, a).await,
        Command::Stations(a) => data::stations(&ctx, a).await,
        Command::Automations(a) => data::automations(&ctx, a).await,
        Command::Tokens(a) => account::tokens(&ctx, a).await,
        Command::Sessions(a) => account::sessions(&ctx, a).await,
        Command::Api(a) => misc::api(&ctx, a).await,
        Command::Config(a) => misc::config(&mut ctx, a),
        Command::Profile(a) => misc::profile(&mut ctx, a),
        Command::Completion(a) => misc::completion(&ctx, a),
        Command::Open(a) => misc::open(&ctx, a),
    }
}

/// One path segment from user input. Ids never contain `/`, `?`, `#`, `%` or `..`.
pub fn seg(id: &str) -> Result<String> {
    let ok = !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));
    if ok {
        Ok(id.to_string())
    } else {
        Err(Error::Usage(format!("'{id}' is not a valid id")))
    }
}

/// `from`/`to` query parameters as instants.
pub fn range_query(range: &TimeRange) -> Result<Vec<(&'static str, String)>> {
    let from = parse_instant_now(&range.from)?;
    let to = parse_instant_now(&range.to)?;
    if from >= to {
        return Err(Error::Usage(format!(
            "--from ({}) must be before --to ({})",
            range.from, range.to
        )));
    }
    Ok(vec![("from", from.to_string()), ("to", to.to_string())])
}

/// A data point id, or `SENSOR/TYPE` / `SENSOR/Name` looked up among the sensor's points.
pub async fn resolve_point(client: &Client, target: &str) -> Result<String> {
    let Some((sensor, wanted)) = target.split_once('/') else {
        return seg(target);
    };
    let detail = client
        .get(&format!("/api/agent/v1/sensors/{}", seg(sensor)?), &[])
        .await?;
    let points = detail["points"].as_array().cloned().unwrap_or_default();
    let hit = points.iter().find(|p| {
        p["type"]
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case(wanted))
            || p["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
            || p["id"].as_str() == Some(wanted)
    });
    match hit.and_then(|p| p["id"].as_str()) {
        Some(id) => Ok(id.to_string()),
        None => {
            let names: Vec<String> = points
                .iter()
                .filter_map(|p| p["type"].as_str().or(p["name"].as_str()))
                .map(|s| s.to_lowercase())
                .collect();
            Err(Error::Api {
                status: 404,
                code: Some("NOT_FOUND".into()),
                message: if names.is_empty() {
                    format!("no data point '{wanted}' on this sensor")
                } else {
                    format!(
                        "no data point '{wanted}' on this sensor, it has: {}",
                        names.join(", ")
                    )
                },
            })
        }
    }
}

/// `items` of a list answer (`{"items": [...]}`) or the array itself.
pub fn items(v: &Value) -> Vec<Value> {
    v["items"]
        .as_array()
        .or_else(|| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// "and 3 more" line for capped lists.
pub fn more_line(ctx: &Ctx, v: &Value) {
    if let Some(n) = v["more"].as_u64().filter(|n| *n > 0) {
        ctx.out.line(&ctx.out.dim(&format!("… and {n} more")));
    }
}

/// A user value: on/off/true/false → bool, numbers → number, else text.
pub fn parse_value(raw: &str) -> Value {
    match raw.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" => return Value::Bool(true),
        "off" | "false" | "no" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(i) = raw.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = raw.parse::<f64>() {
        if f.is_finite() {
            return Value::from(f);
        }
    }
    Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments() {
        assert!(seg("550e8400-e29b-41d4-a716-446655440000").is_ok());
        for bad in ["", "..", "a/b", "a?b", "a%2f", "a b"] {
            assert_eq!(seg(bad).unwrap_err().exit_code(), 2, "{bad}");
        }
    }

    #[test]
    fn values() {
        assert_eq!(parse_value("on"), Value::Bool(true));
        assert_eq!(parse_value("OFF"), Value::Bool(false));
        assert_eq!(parse_value("21.5"), Value::from(21.5));
        assert_eq!(parse_value("-3"), Value::from(-3));
        assert_eq!(parse_value("eco"), Value::from("eco"));
        assert_eq!(parse_value("NaN"), Value::from("NaN"));
    }
}
