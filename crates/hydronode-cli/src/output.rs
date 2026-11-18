//! Tables, colors and JSON. Colors only on a terminal and without `NO_COLOR`.

use std::io::{IsTerminal, Write};

use owo_colors::OwoColorize;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub struct Out {
    pub json: bool,
    pub color: bool,
}

pub fn color_allowed(is_tty: bool) -> bool {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
    is_tty && !no_color && !dumb
}

impl Out {
    pub fn new(json: bool) -> Out {
        Out {
            json,
            color: !json && color_allowed(std::io::stdout().is_terminal()),
        }
    }

    pub fn print_json(&self, v: &Value) {
        let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| "null".into());
        println_safe(&text);
    }

    pub fn table(&self, headers: &[&str], rows: &[Vec<String>]) {
        print_safe(&render_table(headers, rows, self.color));
    }

    pub fn line(&self, text: &str) {
        println_safe(text);
    }

    /// `key  value` lines, keys aligned.
    pub fn fields(&self, pairs: &[(&str, String)]) {
        print_safe(&render_fields(pairs, self.color));
    }

    pub fn bold(&self, s: &str) -> String {
        if self.color {
            s.bold().to_string()
        } else {
            s.to_string()
        }
    }

    pub fn dim(&self, s: &str) -> String {
        if self.color {
            s.dimmed().to_string()
        } else {
            s.to_string()
        }
    }

    /// Status words in traffic-light colors.
    pub fn status(&self, s: &str) -> String {
        paint_status(s, self.color)
    }
}

pub fn paint_status(s: &str, color: bool) -> String {
    if !color {
        return s.to_string();
    }
    match s.to_ascii_uppercase().as_str() {
        "HEALTHY" | "OK" | "ACCEPTED" | "CONFIRMED" | "INSTALLED" | "ON" | "YES" => {
            s.green().to_string()
        }
        "DELAYED" | "WARN" | "STANDBY" | "PENDING" | "QUEUED" | "MEDIUM" => s.yellow().to_string(),
        "OFFLINE" | "REJECTED" | "FAILED" | "NOT_POSSIBLE" | "HIGH" | "CRITICAL"
        | "ROLLED_BACK" => s.red().to_string(),
        _ => s.to_string(),
    }
}

/// Visible width: ANSI sequences do not count.
fn width(s: &str) -> usize {
    let mut w = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c.is_ascii_alphabetic() {
                in_esc = false;
            }
        } else if c == '\u{1b}' {
            in_esc = true;
        } else {
            w += 1;
        }
    }
    w
}

/// Server text (names, messages, payloads) without control characters, so a sensor or station
/// name cannot move the cursor, set the window title or write the clipboard (ANSI/OSC).
/// Only plain colour codes (`ESC [ … m`, what the tables use themselves) survive. Line breaks
/// and tabs become spaces.
pub fn clean(t: &str) -> String {
    if !t.chars().any(char::is_control) {
        return t.to_string();
    }
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\u{1b}' && chars.get(i + 1) == Some(&'[') {
            let mut j = i + 2;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ';') {
                j += 1;
            }
            if chars.get(j) == Some(&'m') {
                out.extend(&chars[i..=j]);
                i = j + 1;
                continue;
            }
        }
        match c {
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
        i += 1;
    }
    out
}

pub fn render_table(headers: &[&str], rows: &[Vec<String>], color: bool) -> String {
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| clean(c)).collect())
        .collect();
    let rows = &rows;
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| width(h)).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(width(cell));
        }
    }
    let mut out = String::new();
    let line = |cells: Vec<String>, out: &mut String| {
        let mut parts = Vec::with_capacity(cols);
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == cols {
                parts.push(cell.clone());
            } else {
                let pad = widths[i].saturating_sub(width(cell));
                parts.push(format!("{cell}{}", " ".repeat(pad)));
            }
        }
        out.push_str(parts.join("  ").trim_end());
        out.push('\n');
    };
    let header: Vec<String> = headers
        .iter()
        .map(|h| {
            if color {
                h.bold().to_string()
            } else {
                h.to_string()
            }
        })
        .collect();
    line(header, &mut out);
    for row in rows {
        let mut cells: Vec<String> = row.iter().take(cols).cloned().collect();
        cells.resize(cols, String::new());
        line(cells, &mut out);
    }
    out
}

pub fn render_fields(pairs: &[(&str, String)], color: bool) -> String {
    let w = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (k, v) in pairs {
        let key = format!("{k:<w$}");
        let key = if color { key.dimmed().to_string() } else { key };
        out.push_str(&format!("{key}  {}\n", clean(v)));
    }
    out
}

/// Print without panicking on a closed pipe (`hn sensors | head -1`).
pub fn print_safe(text: &str) {
    let mut out = std::io::stdout().lock();
    if out.write_all(text.as_bytes()).is_err() {
        std::process::exit(0);
    }
    let _ = out.flush();
}

pub fn println_safe(text: &str) {
    print_safe(&format!("{text}\n"));
}

// ---- small formatting helpers for API values

/// String field, `-` when missing.
pub fn s(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(x) if x.is_empty() => "-".into(),
        Value::String(x) => clean(x),
        Value::Bool(b) => if *b { "on" } else { "off" }.into(),
        Value::Number(n) => num(n.as_f64().unwrap_or(0.0)),
        other => other.to_string(),
    }
}

/// Number without trailing zeros: 21.40 → 21.4, 48.0 → 48.
pub fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let t = format!("{x:.3}");
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Value with unit: `21.4 °C`, `on`.
pub fn with_unit(value: &Value, unit: &Value) -> String {
    let v = s(value);
    match unit.as_str() {
        Some(u) if !u.is_empty() && v != "-" => format!("{v} {u}"),
        _ => v,
    }
}

/// `2026-11-17T04:10:00Z` → `2026-11-17 04:10` in the zone the server sent.
pub fn time(v: &Value) -> String {
    let Some(t) = v.as_str() else {
        return "-".into();
    };
    if t.len() >= 16 && t.as_bytes().get(10) == Some(&b'T') {
        format!("{} {}", &t[..10], &t[11..16])
    } else {
        t.to_string()
    }
}

/// Sparkline of numbers: ▁▂▃▄▅▆▇█.
pub fn sparkline(values: &[f64]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if values.is_empty() {
        return String::new();
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = max - min;
    values
        .iter()
        .map(|v| {
            if span <= f64::EPSILON {
                BARS[3]
            } else {
                BARS[(((v - min) / span) * 7.0).round() as usize]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_text_cannot_drive_the_terminal() {
        let evil = "Bench\u{1b}]52;c;cm0gLXJmIC8=\u{7}\u{1b}[2J\nA\tB";
        let t = render_table(&["NAME"], &[vec![evil.into()]], false);
        assert!(!t.contains('\u{1b}') && !t.contains('\u{7}'), "{t:?}");
        assert!(t.contains("Bench"));
        assert_eq!(t.lines().count(), 2, "{t:?}");
        let f = render_fields(&[("name", evil.into())], false);
        assert!(!f.contains('\u{1b}'), "{f:?}");
        assert_eq!(
            s(&Value::String(evil.into()))
                .chars()
                .filter(|c| c.is_control())
                .count(),
            0
        );
    }

    #[test]
    fn table_snapshot() {
        let rows = vec![
            vec![
                "550e8400".into(),
                "Greenhouse climate".into(),
                "HEALTHY".into(),
            ],
            vec!["770e8400".into(), "Tank".into(), "OFFLINE".into()],
        ];
        insta::assert_snapshot!(render_table(&["ID", "NAME", "STATUS"], &rows, false));
    }

    #[test]
    fn colored_table_keeps_alignment() {
        let rows = vec![vec![paint_status("OFFLINE", true), "x".into()]];
        let text = render_table(&["STATUS", "N"], &rows, true);
        let last = text.lines().last().unwrap();
        assert!(last.contains("\u{1b}["));
        assert_eq!(width(last), "OFFLINE  x".len());
    }

    #[test]
    fn numbers_and_sparkline() {
        assert_eq!(num(21.40), "21.4");
        assert_eq!(num(48.0), "48");
        assert_eq!(num(-0.125), "-0.125");
        assert_eq!(sparkline(&[1.0, 2.0, 3.0, 8.0]), "▁▂▃█");
        assert_eq!(
            time(&Value::String("2026-11-17T04:10:00Z".into())),
            "2026-11-17 04:10"
        );
    }

    #[test]
    fn color_rules() {
        assert!(!color_allowed(false));
    }
}
