//! values (summary, raw, periods) and values compare.

use hydronode_api::time::parse_duration;
use hydronode_api::Result;
use serde_json::Value;

use super::{range_query, resolve_point};
use crate::cli::{ValuesArgs, ValuesCmd};
use crate::ctx::Ctx;
use crate::output::{num, s, sparkline, time, with_unit};

pub async fn run(ctx: &Ctx, args: ValuesArgs) -> Result<()> {
    let client = ctx.client()?;
    if let Some(ValuesCmd::Compare {
        targets,
        range,
        shift,
    }) = args.cmd
    {
        let mut query = range_query(&range)?;
        let mut ids = Vec::new();
        for t in &targets {
            ids.push(resolve_point(&client, t).await?);
        }
        if let Some(shift) = &shift {
            parse_duration(shift)?;
            if ids.len() != 1 {
                return Err(hydronode_api::Error::Usage(
                    "--shift compares one data point with itself, give exactly one".into(),
                ));
            }
            query.push(("shift", shift.clone()));
        }
        query.insert(0, ("points", ids.join(",")));
        let v = client.get("/api/agent/v1/compare", &query).await?;
        if ctx.out.json {
            ctx.out.print_json(&v);
        } else {
            compare(ctx, &v);
        }
        return Ok(());
    }

    let target = args.target.expect("clap requires TARGET");
    let mut query = range_query(&args.range)?;
    let point = resolve_point(&client, &target).await?;
    let (path, mode) = if let Some(n) = args.below {
        query.push(("below", num(n)));
        ("periods", "periods")
    } else if let Some(n) = args.above {
        query.push(("above", num(n)));
        ("periods", "periods")
    } else {
        if args.raw {
            query.push(("raw", "true".into()));
        }
        ("summary", if args.raw { "raw" } else { "summary" })
    };
    let v = client
        .get(&format!("/api/agent/v1/points/{point}/{path}"), &query)
        .await?;
    if ctx.out.json {
        ctx.out.print_json(&v);
        return Ok(());
    }
    match mode {
        "periods" => periods(ctx, &v),
        "raw" => raw(ctx, &v),
        _ => summary(ctx, &v),
    }
    Ok(())
}

fn title(v: &Value) -> String {
    let sensor = s(&v["point"]["sensor"]["name"]);
    if sensor == "-" {
        s(&v["point"]["name"])
    } else {
        format!("{} ({sensor})", s(&v["point"]["name"]))
    }
}

fn summary(ctx: &Ctx, v: &Value) {
    let out = &ctx.out;
    let unit = &v["unit"];
    out.line(&format!(
        "{}  {}",
        out.bold(&title(v)),
        out.dim(&format!(
            "{} → {}, {} steps",
            time(&v["from"]),
            time(&v["to"]),
            s(&v["resolution"])
        ))
    ));
    if v["coverage"].as_f64() == Some(0.0) || v["avg"].is_null() {
        out.line("No data in this time.");
        return;
    }
    let mut fields = vec![
        (
            "min",
            format!(
                "{}  at {}",
                with_unit(&v["min"]["value"], unit),
                time(&v["min"]["at"])
            ),
        ),
        (
            "max",
            format!(
                "{}  at {}",
                with_unit(&v["max"]["value"], unit),
                time(&v["max"]["at"])
            ),
        ),
        ("avg", with_unit(&v["avg"], unit)),
    ];
    if let Some(t) = v["trendPerDay"].as_f64() {
        let u = unit.as_str().map(|u| format!(" {u}")).unwrap_or_default();
        fields.push((
            "trend",
            format!("{}{}{u} per day", if t >= 0.0 { "+" } else { "" }, num(t)),
        ));
    }
    if let Some(c) = v["coverage"].as_f64() {
        fields.push(("coverage", format!("{} %", num((c * 100.0).round()))));
    }
    let gaps = v["gaps"].as_array().map(|g| g.len()).unwrap_or(0);
    if gaps > 0 {
        fields.push(("gaps", gaps.to_string()));
    }
    let avgs: Vec<f64> = v["buckets"]
        .as_array()
        .map(|b| b.iter().filter_map(|x| x["avg"].as_f64()).collect())
        .unwrap_or_default();
    if avgs.len() > 1 {
        fields.push(("shape", sparkline(&avgs)));
    }
    out.fields(&fields);
}

fn raw(ctx: &Ctx, v: &Value) {
    let unit = &v["unit"];
    let rows: Vec<Vec<String>> = v["raw"]
        .as_array()
        .map(|r| {
            r.iter()
                .map(|x| vec![s(&x["at"]), with_unit(&x["value"], unit)])
                .collect()
        })
        .unwrap_or_default();
    if rows.is_empty() {
        ctx.out.line("No readings in this time.");
        return;
    }
    ctx.out.table(&["AT", "VALUE"], &rows);
}

fn periods(ctx: &Ctx, v: &Value) {
    let out = &ctx.out;
    out.line(&format!(
        "{}  {}",
        out.bold(&s(&v["point"]["name"])),
        out.dim(&format!("{} {}", s(&v["condition"]), s(&v["unit"])))
    ));
    let rows: Vec<Vec<String>> = v["periods"]
        .as_array()
        .map(|p| {
            p.iter()
                .map(|x| {
                    vec![
                        time(&x["from"]),
                        time(&x["to"]),
                        format!("{} min", s(&x["minutes"])),
                    ]
                })
                .collect()
        })
        .unwrap_or_default();
    if rows.is_empty() {
        out.line("Never in this time.");
    } else {
        out.table(&["FROM", "TO", "LENGTH"], &rows);
        super::more_line(ctx, v);
    }
    let share = v["share"].as_f64().unwrap_or(0.0);
    out.line(&format!(
        "total {} min, {} % of the time",
        s(&v["totalMinutes"]),
        num((share * 100.0).round())
    ));
}

fn compare(ctx: &Ctx, v: &Value) {
    let series = v["series"].as_array().cloned().unwrap_or_default();
    let rows: Vec<Vec<String>> = series
        .iter()
        .map(|x| {
            let u = &x["unit"];
            vec![
                s(&x["label"]),
                with_unit(&x["min"], u),
                with_unit(&x["avg"], u),
                with_unit(&x["max"], u),
            ]
        })
        .collect();
    ctx.out.table(&["SERIES", "MIN", "AVG", "MAX"], &rows);
    let diffs = v["differences"].as_array().cloned().unwrap_or_default();
    for d in diffs {
        let a = d["a"].as_u64().unwrap_or(0) as usize;
        let b = d["b"].as_u64().unwrap_or(1) as usize;
        let name = |i: usize| series.get(i).map(|x| s(&x["label"])).unwrap_or_default();
        let unit = series
            .first()
            .map(|x| x["unit"].clone())
            .unwrap_or(Value::Null);
        ctx.out.line(&format!(
            "{} vs {}: avg {}, max {}",
            name(a),
            name(b),
            signed(&d["avgDelta"], &unit),
            signed(&d["maxDelta"], &unit)
        ));
    }
}

fn signed(v: &Value, unit: &Value) -> String {
    match v.as_f64() {
        Some(x) => {
            let sign = if x > 0.0 { "+" } else { "" };
            with_unit(&Value::String(format!("{sign}{}", num(x))), unit)
        }
        None => "-".into(),
    }
}
