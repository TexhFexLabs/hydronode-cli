//! Device signature against the shared vectors, and the time parser.

use hydronode_api::device::{format_value, round_payload, rounds_envelope, sign, value_payload};
use hydronode_api::time::{parse_duration, parse_instant};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("signature-vectors.json")).unwrap()
}

#[test]
fn every_value_vector() {
    let v = vectors();
    for case in v["values"].as_array().unwrap() {
        let input = case["input"].as_f64().unwrap();
        assert_eq!(
            format_value(input).unwrap(),
            case["text"].as_str().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn every_single_vector() {
    let v = vectors();
    let (secret, id) = (
        v["secret"].as_str().unwrap(),
        v["sensorId"].as_str().unwrap(),
    );
    for case in v["single"].as_array().unwrap() {
        let ts = case["timestamp"].as_i64().unwrap();
        let payload = value_payload(
            id,
            case["type"].as_str().unwrap(),
            case["value"].as_f64().unwrap(),
            ts,
        )
        .unwrap();
        assert_eq!(payload, case["canonical"].as_str().unwrap());
        assert_eq!(
            sign(secret, &format!("{payload}{ts}")),
            case["signature"].as_str().unwrap()
        );
    }
}

fn values_of(case: &Value) -> Vec<(String, f64)> {
    case["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["type"].as_str().unwrap().to_string(),
                v["value"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_round_vector() {
    let v = vectors();
    let (secret, id) = (
        v["secret"].as_str().unwrap(),
        v["sensorId"].as_str().unwrap(),
    );
    for case in v["round"].as_array().unwrap() {
        let ts = case["timestamp"].as_i64().unwrap();
        let payload = round_payload(id, ts, &values_of(case)).unwrap();
        assert_eq!(payload, case["canonical"].as_str().unwrap());
        assert_eq!(
            sign(secret, &format!("{payload}{ts}")),
            case["signature"].as_str().unwrap()
        );
    }
}

#[test]
fn every_rounds_vector() {
    let v = vectors();
    let (secret, id) = (
        v["secret"].as_str().unwrap(),
        v["sensorId"].as_str().unwrap(),
    );
    for case in v["rounds"].as_array().unwrap() {
        let mut signatures = Vec::new();
        for round in case["rounds"].as_array().unwrap() {
            let ts = round["timestamp"].as_i64().unwrap();
            let payload = round_payload(id, ts, &values_of(round)).unwrap();
            assert_eq!(payload, round["canonical"].as_str().unwrap());
            let s = sign(secret, &format!("{payload}{ts}"));
            assert_eq!(s, round["signature"].as_str().unwrap());
            signatures.push(s);
        }
        let sent = case["sent"].as_i64().unwrap();
        let envelope = rounds_envelope(id, sent, &signatures);
        assert_eq!(envelope, case["envelope"].as_str().unwrap());
        assert_eq!(
            sign(secret, &format!("{envelope}{sent}")),
            case["signature"].as_str().unwrap()
        );
    }
}

#[test]
fn every_raw_vector() {
    let v = vectors();
    let secret = v["secret"].as_str().unwrap();
    for case in v["raw"].as_array().unwrap() {
        let ts = case["timestamp"].as_i64().unwrap();
        assert_eq!(
            sign(
                secret,
                &format!("{}{ts}", case["payload"].as_str().unwrap())
            ),
            case["signature"].as_str().unwrap()
        );
    }
}

#[test]
fn rejects_bad_values_and_types() {
    assert!(format_value(f64::NAN).is_err());
    assert!(format_value(1e40).is_err());
    assert!(round_payload("id", 1730638800, &[("temperature".into(), 1.0)]).is_err());
    assert!(round_payload("id", 1730638800, &[]).is_err());
    assert!(round_payload(
        "id",
        1730638800,
        &[("TEMPERATURE".into(), 1.0), ("TEMPERATURE".into(), 2.0)]
    )
    .is_err());
}

#[test]
fn time_parser() {
    use jiff::tz::TimeZone;
    let tz = TimeZone::fixed(jiff::tz::offset(1));
    let now: jiff::Timestamp = "2026-11-17T12:00:00Z".parse().unwrap();
    let p = |s: &str| parse_instant(s, now, &tz).map(|t| t.to_string());

    assert_eq!(p("24h").unwrap(), "2026-11-16T12:00:00Z");
    assert_eq!(p("7d").unwrap(), "2026-11-10T12:00:00Z");
    assert_eq!(p("90m").unwrap(), "2026-11-17T10:30:00Z");
    assert_eq!(p("30s").unwrap(), "2026-11-17T11:59:30Z");
    assert_eq!(p("2w").unwrap(), "2026-11-03T12:00:00Z");
    assert_eq!(p("now").unwrap(), "2026-11-17T12:00:00Z");
    // a date is midnight in the local zone
    assert_eq!(p("2026-11-01").unwrap(), "2026-10-31T23:00:00Z");
    assert_eq!(p("2026-11-01T10:00+01:00").unwrap(), "2026-11-01T09:00:00Z");
    assert_eq!(p("2026-11-01T10:00:00Z").unwrap(), "2026-11-01T10:00:00Z");
    assert_eq!(p("2026-11-01T10:00").unwrap(), "2026-11-01T09:00:00Z");

    for bad in ["7x", "", "yesterday", "-5d", "2026-13-01", "d7"] {
        let err = p(bad).unwrap_err();
        assert_eq!(err.exit_code(), 2, "{bad}");
    }
}

#[test]
fn duration_parser() {
    assert_eq!(parse_duration("30s").unwrap(), 30);
    assert_eq!(parse_duration("5m").unwrap(), 300);
    assert_eq!(parse_duration("7d").unwrap(), 604_800);
    assert_eq!(parse_duration("45").unwrap(), 45);
    assert!(parse_duration("5x").is_err());
    assert!(parse_duration("0s").is_err());
}
