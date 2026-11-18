//! `hn watch`: live values over `/ws/live/v1` (`watch-dashboard`, LIVE_UPDATES.md §4).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::IsTerminal;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hydronode_api::client::Client;
use hydronode_api::{Error, Result};
use jiff::Timestamp;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Cell, Row, Table};
use ratatui::Frame;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::cli::WatchArgs;
use crate::ctx::Ctx;
use crate::output::num;

/// One value as it arrived.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub sensor_id: String,
    pub sensor: String,
    pub key: String,
    pub value: Value,
    pub at: String,
}

/// What the full-screen view shows: newest value per sensor and type.
#[derive(Debug, Default)]
pub struct State {
    pub rows: BTreeMap<(String, String), Reading>,
    pub status: String,
    pub frames: u64,
}

impl State {
    pub fn apply(&mut self, readings: Vec<Reading>) {
        self.frames += 1;
        for r in readings {
            self.rows.insert((r.sensor.clone(), r.key.clone()), r);
        }
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Number(n) => num(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => if *b { "on" } else { "off" }.into(),
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

fn age(at: &str, now: Timestamp) -> String {
    at.parse::<Timestamp>()
        .map(|t| hydronode_api::time::ago(t, now))
        .unwrap_or_else(|_| "-".into())
}

pub fn render(frame: &mut Frame, state: &State, now: Timestamp) {
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Line::from(format!(
            "HydroNode live  {} values from {} sensors",
            state.rows.len(),
            state
                .rows
                .keys()
                .map(|(s, _)| s)
                .collect::<HashSet<_>>()
                .len()
        ))
        .style(Style::default().add_modifier(Modifier::BOLD)),
        head,
    );
    let rows: Vec<Row> = state
        .rows
        .values()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.sensor.clone()),
                Cell::from(r.key.clone()),
                Cell::from(value_text(&r.value)),
                Cell::from(age(&r.at, now)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(18),
            Constraint::Percentage(12),
        ],
    )
    .header(
        Row::new(vec!["SENSOR", "TYPE", "VALUE", "AGE"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    );
    frame.render_widget(table, body);
    frame.render_widget(
        Line::from(format!("{}  q quits", state.status))
            .style(Style::default().add_modifier(Modifier::DIM)),
        foot,
    );
}

/// `reading` frame → readings with sensor names.
pub fn parse_reading(frame: &Value, names: &HashMap<String, String>) -> Vec<Reading> {
    let sensor_id = frame["sensorId"].as_str().unwrap_or_default().to_string();
    let sensor = names
        .get(&sensor_id)
        .cloned()
        .unwrap_or_else(|| sensor_id.clone());
    let at = frame["at"].as_str().unwrap_or_default().to_string();
    frame["values"]
        .as_array()
        .map(|vals| {
            vals.iter()
                .map(|v| {
                    let mut key = v["k"].as_str().unwrap_or("?").to_string();
                    if let Some(c) = v["c"].as_str() {
                        key = format!("{key}/{c}");
                    }
                    Reading {
                        sensor_id: sensor_id.clone(),
                        sensor: sensor.clone(),
                        key,
                        value: v["v"].clone(),
                        at: at.clone(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn live_url(base: &Url) -> Result<Url> {
    let mut url = base
        .join("/ws/live/v1")
        .map_err(|e| Error::Other(e.to_string()))?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|_| Error::Other("cannot build the live URL".into()))?;
    Ok(url)
}

enum Event {
    Readings(Vec<Reading>),
    Status(String),
}

/// Keeps the socket up: auth, watch, readings; reconnects on 4401 and network drops.
async fn feed(
    client: &Client,
    names: HashMap<String, String>,
    only: Option<HashSet<String>>,
    tx: mpsc::Sender<Event>,
) -> Result<()> {
    let url = live_url(client.base())?;
    let once = std::env::var_os("HYDRONODE_WATCH_ONCE").is_some();
    let mut backoff = 1u64;
    loop {
        let token = client.access_token().await?;
        let _ = tx.send(Event::Status("connecting".into())).await;
        let (mut ws, _) = match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok(c) => c,
            Err(e) => {
                if once {
                    return Err(Error::Other(format!("live connection failed: {e}")));
                }
                let _ = tx
                    .send(Event::Status(format!("offline, retrying in {backoff}s")))
                    .await;
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
                continue;
            }
        };
        ws.send(Message::text(
            json!({"type": "auth", "token": token}).to_string(),
        ))
        .await
        .map_err(|e| Error::Other(e.to_string()))?;
        let mut close_code: Option<u16> = None;
        while let Some(msg) = ws.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(_) => break,
            };
            match msg {
                Message::Text(text) => {
                    let v: Value = serde_json::from_str(text.as_str()).unwrap_or(Value::Null);
                    match v["type"].as_str() {
                        Some("ready") => {
                            ws.send(Message::text(
                                json!({"type": "watch-dashboard", "console": false}).to_string(),
                            ))
                            .await
                            .map_err(|e| Error::Other(e.to_string()))?;
                        }
                        Some("watching-dashboard") => {
                            backoff = 1;
                            let _ = tx.send(Event::Status("live".into())).await;
                        }
                        Some("reading") => {
                            let id = v["sensorId"].as_str().unwrap_or_default();
                            if only.as_ref().is_some_and(|o| !o.contains(id)) {
                                continue;
                            }
                            let readings = parse_reading(&v, &names);
                            if tx.send(Event::Readings(readings)).await.is_err() {
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }
                Message::Close(frame) => {
                    close_code = frame.map(|f| u16::from(f.code));
                    break;
                }
                _ => {}
            }
        }
        match close_code {
            Some(4400) => return Err(Error::Other("the server refused a message (4400)".into())),
            Some(4403) => return Err(Error::Other("the account is gone (4403)".into())),
            Some(4429) => {
                return Err(Error::Api {
                    status: 429,
                    code: None,
                    message: "too many live connections".into(),
                })
            }
            _ => {}
        }
        if once {
            return Ok(());
        }
        // 4401 = token ran out: access_token() refreshes it on the next round
        let _ = tx.send(Event::Status("reconnecting".into())).await;
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

pub async fn run(ctx: &Ctx, args: WatchArgs) -> Result<()> {
    let client = ctx.client()?;
    client.access_token().await?;
    let list = client.get("/api/sensors", &[]).await?;
    let names: HashMap<String, String> = list
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x["id"].as_str()?.to_string(),
                        x["name"].as_str().unwrap_or("?").to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let only = if args.sensors.is_empty() {
        None
    } else {
        let mut set = HashSet::new();
        for wanted in &args.sensors {
            let hit = names
                .iter()
                .find(|(id, name)| *id == wanted || name.eq_ignore_ascii_case(wanted));
            match hit {
                Some((id, _)) => {
                    set.insert(id.clone());
                }
                None => {
                    return Err(Error::Usage(format!(
                        "no sensor '{wanted}', see '{} sensors'",
                        ctx.hn()
                    )))
                }
            }
        }
        Some(set)
    };

    let (tx, mut rx) = mpsc::channel::<Event>(256);
    let plain = args.plain || ctx.out.json || !std::io::stdout().is_terminal();
    if plain {
        let feeder = feed(&client, names, only, tx);
        let printer = async {
            while let Some(ev) = rx.recv().await {
                if let Event::Readings(rs) = ev {
                    for r in rs {
                        if ctx.out.json {
                            ctx.out.line(
                                &json!({"at": r.at, "sensorId": r.sensor_id, "sensor": r.sensor, "type": r.key, "value": r.value})
                                    .to_string(),
                            );
                        } else {
                            ctx.out.line(&format!(
                                "{}  {}  {}  {}",
                                r.at,
                                r.sensor,
                                r.key,
                                value_text(&r.value)
                            ));
                        }
                    }
                }
            }
        };
        let (res, _) = tokio::join!(feeder, printer);
        return res;
    }
    tui(&client, names, only, tx, rx).await
}

async fn tui(
    client: &Client,
    names: HashMap<String, String>,
    only: Option<HashSet<String>>,
    tx: mpsc::Sender<Event>,
    mut rx: mpsc::Receiver<Event>,
) -> Result<()> {
    use ratatui::crossterm::event::{self, Event as TermEvent, KeyCode, KeyModifiers};

    let (key_tx, mut key_rx) = mpsc::channel::<()>(1);
    std::thread::spawn(move || loop {
        if let Ok(true) = event::poll(Duration::from_millis(200)) {
            if let Ok(TermEvent::Key(k)) = event::read() {
                let quit = matches!(k.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (k.code == KeyCode::Char('c')
                        && k.modifiers.contains(KeyModifiers::CONTROL));
                if quit {
                    let _ = key_tx.blocking_send(());
                    return;
                }
            }
        }
        if key_tx.is_closed() {
            return;
        }
    });

    let mut terminal = ratatui::init();
    let mut state = State {
        status: "connecting".into(),
        ..State::default()
    };
    let feeder = feed(client, names, only, tx);
    tokio::pin!(feeder);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let result = loop {
        let _ = terminal.draw(|f| render(f, &state, Timestamp::now()));
        tokio::select! {
            r = &mut feeder => break r,
            _ = key_rx.recv() => break Ok(()),
            ev = rx.recv() => match ev {
                Some(Event::Readings(rs)) => state.apply(rs),
                Some(Event::Status(s)) => state.status = s,
                None => break Ok(()),
            },
            _ = tick.tick() => {}
        }
    };
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn tui_snapshot() {
        let names: HashMap<String, String> =
            [("s1".to_string(), "Greenhouse climate".to_string())].into();
        let frame = json!({"type": "reading", "sensorId": "s1", "at": "2026-11-17T12:00:01Z",
            "values": [{"k": "TEMPERATURE", "c": null, "v": 21.4}, {"k": "HUMIDITY", "c": null, "v": 48},
                       {"k": "SWITCH", "c": "fan", "v": true}]});
        let mut state = State {
            status: "live".into(),
            ..State::default()
        };
        state.apply(parse_reading(&frame, &names));
        let now: Timestamp = "2026-11-17T12:00:31Z".parse().unwrap();
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal.draw(|f| render(f, &state, now)).unwrap();
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn live_url_schemes() {
        assert_eq!(
            live_url(&Url::parse("https://example.com").unwrap())
                .unwrap()
                .as_str(),
            "wss://example.com/ws/live/v1"
        );
        assert_eq!(
            live_url(&Url::parse("http://127.0.0.1:8080").unwrap())
                .unwrap()
                .as_str(),
            "ws://127.0.0.1:8080/ws/live/v1"
        );
    }
}
