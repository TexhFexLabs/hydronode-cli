//! C4: data commands against a fake backend.

mod common;
use common::*;

use serde_json::json;
use wiremock::matchers::{body_json, body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn server_with_session() -> (MockServer, Env) {
    let server = MockServer::start().await;
    let env = Env::with_credential(&server.uri(), session());
    (server, env)
}

fn sensors_body() -> serde_json::Value {
    json!([
        {"id": SENSOR, "name": "Greenhouse climate", "isOwner": true, "accessLevel": null, "active": true,
         "monitoring": {"status": "HEALTHY"},
         "latestValues": [{"type": "TEMPERATURE", "value": 21.4, "timestamp": "2026-11-17T11:58:00Z"}]},
        {"id": "770e8400-e29b-41d4-a716-446655440000", "name": "Tank", "isOwner": false, "accessLevel": "READ",
         "active": true, "monitoring": {"status": "OFFLINE"}, "latestValues": []}
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn sensors_list_table_and_json() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path("/api/sensors"))
        .and(header("authorization", "Bearer eyJ.fake.access"))
        // Without it the API hides virtual, MQTT and push sensors (DATA_POINTS.md §2).
        .and(header("x-hydronode-capabilities", "data1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sensors_body()))
        .mount(&server)
        .await;
    let out = run_args(&env, &["sensors", "list"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("sensors_list_plain", stdout(&out));

    let out = run_args(&env, &["sensors", "--json"]).await;
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v, sensors_body());
}

#[tokio::test(flavor = "multi_thread")]
async fn sensors_show_uses_agent_api() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/agent/v1/sensors/{SENSOR}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": SENSOR, "name": "Greenhouse climate", "status": "HEALTHY", "ago": "2m",
            "project": {"id": "p1", "name": "Home"}, "pool": {"id": "x", "name": "Bench A"},
            "battery": {"percent": 81, "voltage": 3.92}, "signal": {"rssi": -67, "kind": "wifi"},
            "firmware": "hydronode/0.9.0", "canEdit": true,
            "points": [
                {"id": POINT, "name": "Temperature", "type": "TEMPERATURE", "kind": "NUMBER", "value": 21.4,
                 "unit": "°C", "ago": "2m", "direction": "IN", "writable": false, "manual": false},
                {"id": "p2", "name": "Fan", "type": "SWITCH", "kind": "BOOL", "value": true,
                 "unit": null, "ago": "5m", "direction": "BOTH", "writable": true, "manual": false}
            ]})))
        .mount(&server)
        .await;
    let out = run_args(&env, &["sensors", "show", SENSOR]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("sensors_show_plain", stdout(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn sensors_create_rename_credentials() {
    let (server, env) = server_with_session().await;
    Mock::given(method("POST"))
        .and(path("/api/sensors"))
        .and(body_json(json!({"name": "Balcony", "outdoor": true})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "sensorId": SENSOR, "secret": "fake_secret_value", "name": "Balcony", "active": true, "outdoor": true})))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/api/sensors/{SENSOR}")))
        .and(body_json(json!({"name": "Balcony east"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": SENSOR, "name": "Balcony east"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/sensors/{SENSOR}/credentials")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sensorId": SENSOR, "secret": "fake_secret_value", "name": "Balcony", "active": true})))
        .mount(&server)
        .await;

    let out = run_args(&env, &["sensors", "create", "Balcony", "--outdoor"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains(SENSOR));
    assert!(
        !stdout(&out).contains("fake_secret_value"),
        "create never prints the secret"
    );
    let out = run_args(
        &env,
        &["sensors", "create", "Balcony", "--outdoor", "--json"],
    )
    .await;
    assert!(!stdout(&out).contains("fake_secret_value"));

    let out = run_args(&env, &["sensors", "rename", SENSOR, "Balcony east"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let out = run_args(&env, &["sensors", "credentials", SENSOR]).await;
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("fake_secret_value"));
}

#[tokio::test(flavor = "multi_thread")]
async fn sensors_delete_confirms() {
    let (server, env) = server_with_session().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/api/sensors/{SENSOR}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"message": "Sensor deleted successfully"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    // no TTY, no --yes: refuse without touching the server
    let out = run_args(&env, &["sensors", "delete", SENSOR]).await;
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("--yes"), "{}", stderr(&out));
    let out = run_args(&env, &["sensors", "delete", SENSOR, "--yes"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn not_found_exits_4() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/agent/v1/sensors/{SENSOR}")))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"error": "NOT_FOUND", "message": "Sensor not found"})),
        )
        .mount(&server)
        .await;
    let out = run_args(&env, &["sensors", "show", SENSOR]).await;
    assert_eq!(code(&out), 4);
    assert!(stderr(&out).starts_with("error: Sensor not found"));
}

fn summary() -> serde_json::Value {
    json!({
    "point": {"id": POINT, "name": "Temperature", "sensor": {"id": SENSOR, "name": "Greenhouse climate"}},
    "unit": "°C", "from": "2026-11-16T12:00:00Z", "to": "2026-11-17T12:00:00Z", "resolution": "5m",
    "min": {"value": 14.2, "at": "2026-11-17T04:10:00Z"}, "max": {"value": 29.8, "at": "2026-11-16T14:05:00Z"},
    "avg": 21.3, "trendPerDay": 0.4, "coverage": 0.97,
    "gaps": [{"from": "2026-11-17T01:00:00Z", "to": "2026-11-17T01:40:00Z"}],
    "buckets": [
        {"from": "2026-11-16T12:00:00Z", "min": 18.0, "avg": 19.0, "max": 20.0},
        {"from": "2026-11-16T18:00:00Z", "min": 20.0, "avg": 24.0, "max": 29.8},
        {"from": "2026-11-17T00:00:00Z", "min": 14.2, "avg": 16.0, "max": 18.0},
        {"from": "2026-11-17T06:00:00Z", "min": 17.0, "avg": 21.0, "max": 25.0}
    ]})
}

#[tokio::test(flavor = "multi_thread")]
async fn values_summary_periods_and_resolution() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/agent/v1/points/{POINT}/summary")))
        .and(query_param("from", "2026-11-10T12:00:00Z"))
        .and(query_param("to", "2026-11-17T12:00:00Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(summary()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/agent/v1/points/{POINT}/periods")))
        .and(query_param("below", "16"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "point": {"id": POINT, "name": "Temperature"}, "unit": "°C", "condition": "below 16",
            "periods": [{"from": "2026-11-17T02:00:00Z", "to": "2026-11-17T04:30:00Z", "minutes": 150}],
            "more": 0, "totalMinutes": 150, "share": 0.10})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/agent/v1/sensors/{SENSOR}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": SENSOR, "name": "Greenhouse climate",
            "points": [{"id": POINT, "name": "Temperature", "type": "TEMPERATURE"}]})))
        .mount(&server)
        .await;

    let out = run_args(
        &env,
        &[
            "values",
            POINT,
            "--from",
            "2026-11-10T12:00:00Z",
            "--to",
            "2026-11-17T12:00:00Z",
        ],
    )
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("values_summary_plain", stdout(&out));

    // SENSOR/TYPE resolves through the sensor's points
    let target = format!("{SENSOR}/temperature");
    let out = run_args(&env, &["values", &target, "--below", "16", "--json"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["totalMinutes"], 150);
}

#[tokio::test(flavor = "multi_thread")]
async fn values_invalid_time_is_usage() {
    let env = Env::with_credential("https://example.com", session());
    let out = run_args(&env, &["values", POINT, "--from", "7x"]).await;
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("not a time"));
    let out = run_args(&env, &["values", POINT, "--below", "1", "--above", "2"]).await;
    assert_eq!(code(&out), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn values_compare_with_shift() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/compare"))
        .and(query_param("points", POINT))
        .and(query_param("shift", "7d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "series": [
                {"point": {"id": POINT, "name": "Temperature"}, "label": "Temperature (now)", "unit": "°C", "min": 14.2, "max": 29.8, "avg": 21.3, "buckets": []},
                {"point": {"id": POINT, "name": "Temperature"}, "label": "Temperature (7d before)", "unit": "°C", "min": 12.0, "max": 25.1, "avg": 19.9, "buckets": []}
            ],
            "differences": [{"a": 0, "b": 1, "avgDelta": 1.4, "maxDelta": 4.7}]})))
        .expect(1)
        .mount(&server)
        .await;
    let out = run_args(&env, &["values", "compare", POINT, "--shift", "7d"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("values_compare_plain", stdout(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn read_token_cannot_write() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let env = Env::with_credential(&server.uri(), pat("hrt_fake_read"));
    let out = run_args(&env, &["set", POINT, "on"]).await;
    assert_eq!(code(&out), 3);
    assert!(
        stderr(&out).contains("hint: This needs a full login. Run 'hn login'."),
        "{}",
        stderr(&out)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn set_and_command() {
    let (server, env) = server_with_session().await;
    Mock::given(method("POST"))
        .and(path(format!("/api/agent/v1/points/{POINT}/value")))
        .and(body_json(json!({"value": true, "waitSeconds": 10})))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"state": "CONFIRMED", "desired": true, "actual": true, "reason": null}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/sensors/{SENSOR}/commands")))
        .and(body_json(
            json!({"command": "relay1", "type": "UINT32", "value": 1400}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"id": "c1", "status": "PENDING"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let out = run_args(&env, &["set", POINT, "on"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("CONFIRMED"));
    let out = run_args(
        &env,
        &["command", SENSOR, "relay1", "1400", "--type", "uint32"],
    )
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn console_lines_and_rejected_filter() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/sensors/{SENSOR}/console/events")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "cursor": 12, "hasMore": false, "retentionHours": 48,
            "events": [
                {"id": 12, "at": "2026-11-17T11:59:00Z", "source": "HTTP", "outcome": "REJECTED", "status": 401, "message": "Invalid signature"},
                {"id": 11, "at": "2026-11-17T11:58:00Z", "source": "HTTP", "outcome": "ACCEPTED", "status": 202, "message": "TEMPERATURE 21.4"}
            ]})))
        .mount(&server)
        .await;
    let out = run_args(&env, &["console", SENSOR]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("console_plain", stdout(&out));
    let out = run_args(&env, &["console", SENSOR, "--rejected"]).await;
    assert!(stdout(&out).contains("Invalid signature"));
    assert!(!stdout(&out).contains("TEMPERATURE 21.4"));
}

#[tokio::test(flavor = "multi_thread")]
async fn status_anomalies_messages_stations_automations() {
    let (server, env) = server_with_session().await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/overview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "projects": [{"id": "p", "name": "Home", "pools": [{"id": "x", "name": "Bench A", "sensors": 4}]}],
            "devices": {"total": 9, "healthy": 6, "delayed": 1, "offline": 1, "standby": 0, "noData": 1},
            "openAnomalies": 2, "unreadMessages": 3,
            "needsAttention": [{"kind": "OFFLINE", "sensor": {"id": SENSOR, "name": "Tank"}, "detail": "Last data 3h ago", "ago": "3h"}]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/anomalies"))
        .and(query_param("state", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "a1", "title": "Temperature spike", "severity": "HIGH", "state": "OPEN", "sensor": {"id": SENSOR, "name": "Greenhouse climate"}, "ago": "1h"}
        ], "more": 0, "next": null})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/messages"))
        .and(query_param("unread", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "m1", "category": "DEVICE", "title": "Tank went quiet", "read": false, "ago": "3h"}
        ], "more": 0, "next": null})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/messages/m1/read"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/agent/v1/messages/read-all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"marked": 5})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/agent/v1/stations/public"))
        .and(query_param("near", "52.5,13.4"))
        .and(query_param("radiusKm", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "s1", "name": "Berlin Mitte", "distanceKm": 1.2, "values": [{"name": "Temperature", "value": 8.4, "unit": "°C", "ago": "4m"}]}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/agent/v1/automations/r1/enabled"))
        .and(body_json(json!({"enabled": false})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "r1", "enabled": false})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let out = run_args(&env, &["status"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    insta::assert_snapshot!("status_plain", stdout(&out));
    let out = run_args(&env, &["anomalies"]).await;
    assert!(
        stdout(&out).contains("Temperature spike"),
        "{}",
        stderr(&out)
    );
    let out = run_args(&env, &["messages", "--unread"]).await;
    assert!(stdout(&out).contains("Tank went quiet"), "{}", stderr(&out));
    let out = run_args(&env, &["messages", "read", "m1"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let out = run_args(&env, &["messages", "read", "--all"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let out = run_args(&env, &["stations", "--near", "52.5,13.4", "--radius", "5"]).await;
    assert!(stdout(&out).contains("Berlin Mitte"), "{}", stderr(&out));
    let out = run_args(&env, &["automations", "disable", "r1"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

#[tokio::test(flavor = "multi_thread")]
async fn export_csv_json_parquet() {
    let (server, env) = server_with_session().await;
    let csv = "sensor_id,sensor_name,type,channel_name,timestamp,value,raw_value,calibrated,origin\n\
        550e8400-e29b-41d4-a716-446655440000,Greenhouse,TEMPERATURE,,2026-11-17T10:00:00Z,21.4,21.4,false,DEVICE\n\
        550e8400-e29b-41d4-a716-446655440000,Greenhouse,TEMPERATURE,,2026-11-17T10:05:00Z,21.6,21.6,false,DEVICE\n";
    Mock::given(method("GET"))
        .and(path("/api/data-transfer/v1/export"))
        .and(query_param("format", "csv"))
        .and(query_param("sensors", SENSOR))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/csv")
                .set_body_string(csv),
        )
        .mount(&server)
        .await;
    let ndjson = "{\"sensor_id\":\"x\",\"value\":21.4}\n{\"sensor_id\":\"x\",\"value\":21.6}\n";
    Mock::given(method("GET"))
        .and(path("/api/data-transfer/v1/export"))
        .and(query_param("format", "ndjson"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ndjson))
        .mount(&server)
        .await;

    let out = run_args(&env, &["export", "--sensor", SENSOR, "--format", "csv"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out), csv);

    let out = run_args(&env, &["export", "--format", "json"]).await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);

    let file = env.dir.path().join("out.parquet");
    let out = run_args(
        &env,
        &[
            "export",
            "--sensor",
            SENSOR,
            "--format",
            "parquet",
            "-o",
            file.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let reader = SerializedFileReader::new(std::fs::File::open(&file).unwrap()).unwrap();
    let meta = reader.metadata();
    assert_eq!(meta.file_metadata().num_rows(), 2);
    let schema = meta.file_metadata().schema_descr();
    assert_eq!(schema.num_columns(), 9);
    assert_eq!(schema.column(5).name(), "value");
    assert_eq!(
        schema.column(5).physical_type(),
        parquet::basic::Type::DOUBLE
    );
    assert_eq!(
        schema.column(0).physical_type(),
        parquet::basic::Type::BYTE_ARRAY
    );
    let _ = body_partial_json(json!({}));
}
