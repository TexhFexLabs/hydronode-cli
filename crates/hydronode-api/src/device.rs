//! The signed device protocol (HMAC-SHA256), as in `hydronode-sdk/PROTOCOL.md`.
//!
//! `message = canonical payload + X-Timestamp`, `X-Signature = Base64(HMAC-SHA256(secret, message))`.
//! Values are float32, printed with two decimals, half up on the exact value, sign kept.

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{Error, Result};

pub const MAX_VALUES_PER_ROUND: usize = 64;

/// The value as HydroNode reads it: `21.125` → `21.13`, `-0.004` → `-0.00`.
pub fn format_value(value: f64) -> Result<String> {
    let f = value as f32;
    if !value.is_finite() || !f.is_finite() {
        return Err(Error::Usage(format!(
            "{value} is not a number that fits a 32-bit float"
        )));
    }
    let bits = f.to_bits();
    let negative = bits >> 31 == 1;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let fraction = (bits & 0x7f_ffff) as u128;
    let (mantissa, shift) = if exponent == 0 {
        (fraction, -149)
    } else {
        (fraction | 0x80_0000, exponent - 150)
    };
    let sign = if negative { "-" } else { "" };
    if shift >= 0 {
        // an integer: mantissa < 2^24, shift <= 104, fits u128
        return Ok(format!("{sign}{}.00", mantissa << shift));
    }
    let cents_scaled = mantissa * 100;
    let down = (-shift) as u32;
    let cents = if down >= 127 {
        0
    } else {
        let den = 1u128 << down;
        let q = cents_scaled / den;
        let rem = cents_scaled % den;
        if rem * 2 >= den {
            q + 1
        } else {
            q
        }
    };
    Ok(format!("{sign}{}.{:02}", cents / 100, cents % 100))
}

/// `A-Z`, `0-9`, `_`, starting with a letter, at most 64 characters.
pub fn valid_type(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

pub fn sign(secret: &str, message: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Canonical text of one value (`POST /api/webhook/sensor-value`).
pub fn value_payload(
    sensor_id: &str,
    value_type: &str,
    value: f64,
    timestamp: i64,
) -> Result<String> {
    check_type(value_type)?;
    Ok(format!(
        r#"{{"sensorId":"{sensor_id}","type":"{value_type}","value":{},"timestamp":{timestamp}}}"#,
        format_value(value)?
    ))
}

/// Canonical text of one round (`POST /api/webhook/sensor-values`); also the request body.
pub fn round_payload(sensor_id: &str, timestamp: i64, values: &[(String, f64)]) -> Result<String> {
    if values.is_empty() || values.len() > MAX_VALUES_PER_ROUND {
        return Err(Error::Usage("send 1 to 64 values".into()));
    }
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::with_capacity(values.len());
    for (t, v) in values {
        check_type(t)?;
        if !seen.insert(t.as_str()) {
            return Err(Error::Usage(format!("type {t} appears twice")));
        }
        items.push(format!(r#"{{"type":"{t}","value":{}}}"#, format_value(*v)?));
    }
    Ok(format!(
        r#"{{"sensorId":"{sensor_id}","timestamp":{timestamp},"values":[{}]}}"#,
        items.join(",")
    ))
}

/// Canonical envelope of a stored-rounds request: send time and the round signatures.
pub fn rounds_envelope(sensor_id: &str, sent: i64, signatures: &[String]) -> String {
    let quoted: Vec<String> = signatures.iter().map(|s| format!("\"{s}\"")).collect();
    format!(
        r#"{{"sensorId":"{sensor_id}","timestamp":{sent},"rounds":[{}]}}"#,
        quoted.join(",")
    )
}

fn check_type(t: &str) -> Result<()> {
    if valid_type(t) {
        Ok(())
    } else {
        Err(Error::Usage(format!(
            "invalid type '{t}': use A-Z, 0-9 and _ like TEMPERATURE"
        )))
    }
}

/// One signed round, ready to send.
#[derive(Debug, Clone)]
pub struct SignedRound {
    pub body: String,
    pub timestamp: i64,
    pub signature: String,
}

pub fn sign_round(
    sensor_id: &str,
    secret: &str,
    timestamp: i64,
    values: &[(String, f64)],
) -> Result<SignedRound> {
    let body = round_payload(sensor_id, timestamp, values)?;
    let signature = sign(secret, &format!("{body}{timestamp}"));
    Ok(SignedRound {
        body,
        timestamp,
        signature,
    })
}

/// Send one signed round to `/api/webhook/sensor-values`. Returns status and answer body.
pub async fn send_round(
    http: &reqwest::Client,
    server: &url::Url,
    sensor_id: &str,
    round: &SignedRound,
) -> Result<(u16, serde_json::Value)> {
    let url = server
        .join("/api/webhook/sensor-values")
        .map_err(|e| Error::Other(e.to_string()))?;
    let resp = http
        .post(url)
        .header("Content-Type", "application/json")
        .header("X-Sensor-Id", sensor_id)
        .header("X-Timestamp", round.timestamp.to_string())
        .header("X-Signature", &round.signature)
        .header(
            "X-Firmware",
            concat!("hydronode-cli/", env!("CARGO_PKG_VERSION")),
        )
        .body(round.body.clone())
        .send()
        .await?;
    let status = resp.status().as_u16();
    let text = crate::client::read_text(resp, crate::client::MAX_SMALL_BODY).await?;
    let body = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    Ok((status, body))
}
