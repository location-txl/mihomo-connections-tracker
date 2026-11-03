use crate::{
    api,
    common::{parse_snapshot, retention_cutoff, Observation, Topology},
    config::Config,
    db::{Database, DetailFilter},
};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde_json::{json, Value};
use std::sync::Arc;

fn time(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

fn topology() -> Topology {
    Topology::parse(json!({"proxies": {
        "ChatGPT": {"type": "Selector", "all": ["日本", "香港"]},
        "自动选择": {"type": "URLTest", "all": ["日本"]},
        "香港": {"type": "Trojan"},
        "DIRECT-IPV4": {"type": "Direct"},
        "REJECT": {"type": "Reject"},
        "拨号": {"type": "Trojan", "dialer-proxy": "香港"}
    }}), json!({"providers": {
        "宝可梦专线": {"proxies": [{"name": "日本", "type": "Shadowsocks"}]},
        "ChatGPT": {"vehicleType": "Compatible", "proxies": [{"name": "香港", "type": "Trojan"}]}
    }}), json!({"version": "test"}), 1).unwrap()
}

fn raw(id: &str, upload: i64, download: i64, start: &str) -> Value {
    json!({"id": id, "upload": upload, "download": download, "start": start,
        "metadata": {"network": "tcp", "host": "ChatGPT.COM.", "destinationIP": "", "sniffHost": ""},
        "chains": ["日本", "自动选择", "ChatGPT"],
        "providerChains": ["宝可梦专线", "", ""], "rule": "RuleSet", "rulePayload": "openai",
        "future_field": {"preserve": true}})
}

fn observations(rows: Vec<Value>) -> Vec<Observation> {
    parse_snapshot(&json!({"connections": rows}).to_string()).unwrap()
}

fn filter(month: &str) -> DetailFilter {
    let start = time(&format!("{month}-01T00:00:00Z")).date_naive();
    DetailFilter {
        from: start.to_string(),
        to: (start + chrono::Months::new(1)).to_string(),
        airport: None,
        group: None,
        domain: None,
        rule: None,
        limit: 50,
        offset: 0,
    }
}

#[test]
fn provider_attribution_nulls_aliases_and_ambiguous_chains() {
    let topo = topology();
    let mut value = raw("1", 7485, 9929, "2026-11-03T17:46:37+08:00");
    let obs = observations(vec![value.clone()]);
    let attribution = topo.attribute(&obs[0].connection, "主机场");
    assert_eq!(attribution.airport.as_deref(), Some("宝可梦专线"));
    assert_eq!(attribution.groups, ["自动选择", "ChatGPT"]);
    for provider in [json!(["", ""]), json!([null, ""]), Value::Null] {
        value["chains"] = json!(["香港", "ChatGPT"]);
        value["providerChains"] = provider;
        let obs = observations(vec![value.clone()]);
        let attribution = topo.attribute(&obs[0].connection, "主机场");
        assert_eq!(attribution.airport.as_deref(), Some("主机场"));
        assert_eq!(attribution.reason, None);
    }
    value["chains"] = json!(["DIRECT-IPV4", "ChatGPT"]);
    value["providerChains"] = json!(["", ""]);
    let obs = observations(vec![value.clone()]);
    let attribution = topo.attribute(&obs[0].connection, "主机场");
    assert_eq!(attribution.route_kind, "direct");
    assert!(attribution.airport.is_none());
    value["chains"] = json!(["日本", "香港", "ChatGPT"]);
    value["providerChains"] = json!(["宝可梦专线", "", ""]);
    let obs = observations(vec![value.clone()]);
    let attribution = topo.attribute(&obs[0].connection, "主机场");
    assert_eq!(attribution.reason, Some("ambiguous_multihop"));
    assert!(attribution.airport.is_none());
    value["providerChains"] = json!([""]);
    assert_eq!(
        topo.attribute(&observations(vec![value])[0].connection, "主机场")
            .reason,
        Some("chain_length_mismatch")
    );
}

#[test]
fn rejects_invalid_snapshots_without_treating_them_as_empty() {
    assert!(parse_snapshot("{}").is_err());
    assert!(parse_snapshot(r#"{"connections": null}"#)
        .unwrap()
        .is_empty());
    let value = raw("1", -1, 0, "2026-11-03T00:00:00Z");
    assert!(parse_snapshot(&json!({"connections": [value]}).to_string()).is_err());
    let value = raw("1", 1, 0, "2026-11-03T00:00:00Z");
    assert!(parse_snapshot(&json!({"connections": [value.clone(), value]}).to_string()).is_err());
}

#[tokio::test]
async fn deltas_cross_days_months_groups_and_domain_queries() {
    let db = Database::open(":memory:").await.unwrap();
    let topo = topology();
    let since = time("2026-10-31T15:59:00Z");
    let start = "2026-10-31T15:59:10Z";
    let first = observations(vec![raw("1", 100, 200, start)]);
    db.ingest(&first, &topo, "主机场", time("2026-10-31T15:59:59Z"), since)
        .await
        .unwrap();
    db.ingest(&first, &topo, "主机场", time("2026-10-31T15:59:59Z"), since)
        .await
        .unwrap();
    db.ingest(
        &observations(vec![raw("1", 150, 300, start)]),
        &topo,
        "主机场",
        time("2026-10-31T16:00:01Z"),
        since,
    )
    .await
    .unwrap();
    let rows = db
        .stats("global", "day", "2026-10-31", "2026-11-02", None)
        .await
        .unwrap();
    assert_eq!(rows[0]["total"], 300);
    assert_eq!(rows[1]["total"], 150);
    let groups = db
        .stats("group", "month", "2026-10-01", "2026-12-01", None)
        .await
        .unwrap();
    assert_eq!(groups.len(), 4);
    assert_eq!(
        groups
            .iter()
            .map(|r| r["total"].as_i64().unwrap())
            .sum::<i64>(),
        900
    );
    let airports = db
        .stats("airport", "month", "2026-10-01", "2026-12-01", None)
        .await
        .unwrap();
    assert_eq!(
        airports
            .iter()
            .map(|r| r["total"].as_i64().unwrap())
            .sum::<i64>(),
        450
    );
    let mut f = filter("2026-11");
    f.group = Some("ChatGPT".into());
    f.airport = Some("宝可梦专线".into());
    let connections = db.connections(&f).await.unwrap();
    assert_eq!(connections["total_count"], 1);
    assert_eq!(connections["items"][0]["total"], 150);
    assert_eq!(connections["items"][0]["observed_download"], 300);
    assert_eq!(
        connections["items"][0]["chains"],
        json!(["日本", "自动选择", "ChatGPT"])
    );
    let domains = db.domains(&f).await.unwrap();
    assert_eq!(domains["items"][0]["domain"], "chatgpt.com");
    assert_eq!(domains["items"][0]["connection_count"], 1);
    assert_eq!(domains["items"][0]["total"], 150);
    f.domain = Some("' OR 1=1 --".into());
    assert_eq!(db.connections(&f).await.unwrap()["total_count"], 0);

    // 无域名的连接按目标 IP 聚合：两条同 IP 连接合并为一行。
    let mut ip = raw("2", 10, 20, start);
    ip["metadata"]["host"] = json!("");
    ip["metadata"]["destinationIP"] = json!("1.2.3.4");
    ip["chains"] = json!(["香港"]);
    ip["providerChains"] = json!([""]);
    let mut ip2 = ip.clone();
    ip2["id"] = json!("3");
    ip2["upload"] = json!(1);
    ip2["download"] = json!(1);
    db.ingest(
        &observations(vec![ip, ip2]),
        &topo,
        "主机场",
        time("2026-10-31T16:00:02Z"),
        since,
    )
    .await
    .unwrap();
    let domains = db.domains(&filter("2026-11")).await.unwrap();
    assert_eq!(domains["items"][0]["domain"], "chatgpt.com");
    assert_eq!(domains["items"][1]["domain"], "1.2.3.4");
    assert_eq!(domains["items"][1]["connection_count"], 2);
    assert_eq!(domains["items"][1]["total"], 32);
}

#[tokio::test]
async fn restart_uses_persistent_baselines_and_counter_reset_is_audited() {
    let path = std::env::temp_dir().join(format!(
        "mihomo-test-{}-{}.db",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let topo = topology();
    let since = time("2026-11-03T01:00:00Z");
    let start = "2026-11-03T00:00:00Z";
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    db.ingest(
        &observations(vec![raw("1", 1000, 2000, start)]),
        &topo,
        "主机场",
        since,
        since,
    )
    .await
    .unwrap();
    db.pool.close().await;
    let db = Database::open(path.to_str().unwrap()).await.unwrap();
    let restarted = time("2026-11-04T01:00:00Z");
    db.begin_gap(restarted.timestamp_millis(), "service_start")
        .await
        .unwrap();
    db.ingest(
        &observations(vec![raw("1", 1100, 2300, start)]),
        &topo,
        "主机场",
        restarted,
        restarted,
    )
    .await
    .unwrap();
    let totals = db
        .stats("global", "month", "2026-11-01", "2026-12-01", None)
        .await
        .unwrap();
    assert_eq!(totals[0]["total"], 400);
    assert_eq!(db.health().await.unwrap()["connected"], true);
    let gaps = db
        .gaps(
            since.timestamp_millis() - 1,
            restarted.timestamp_millis() + 1,
        )
        .await
        .unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0]["started_at"], since.timestamp_millis());
    assert_eq!(gaps[0]["ended_at"], restarted.timestamp_millis());
    db.ingest(
        &observations(vec![raw("1", 10, 20, start)]),
        &topo,
        "主机场",
        restarted,
        restarted,
    )
    .await
    .unwrap();
    let totals = db
        .stats("global", "day", "2026-11-04", "2026-11-05", None)
        .await
        .unwrap();
    assert_eq!(totals[0]["total"], 400);
    assert_eq!(
        db.anomalies(None, Some("counter_reset"), 50, 0)
            .await
            .unwrap()["items"][0]["occurrences"],
        1
    );
    db.pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn transaction_failure_does_not_advance_baseline_or_partial_statistics() {
    let db = Database::open(":memory:").await.unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_total BEFORE INSERT ON daily_totals WHEN NEW.dimension = 'airport'
        BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    let now = time("2026-11-03T00:00:00Z");
    let obs = observations(vec![raw("1", 50, 100, "2026-11-03T00:00:00Z")]);
    assert!(db
        .ingest(&obs, &topology(), "主机场", now, now)
        .await
        .is_err());
    assert!(db.health().await.unwrap()["last_success_at"].is_null());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM connections")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(db
        .stats("global", "day", "2026-11-03", "2026-11-04", None)
        .await
        .unwrap()
        .is_empty());
    sqlx::query("DROP TRIGGER reject_total")
        .execute(&db.pool)
        .await
        .unwrap();
    db.ingest(&obs, &topology(), "主机场", now, now)
        .await
        .unwrap();
    assert_eq!(
        db.stats("global", "day", "2026-11-03", "2026-11-04", None)
            .await
            .unwrap()[0]["total"],
        150
    );
}

#[tokio::test]
async fn retention_keeps_totals_anomalies_and_active_baselines() {
    let db = Database::open(":memory:").await.unwrap();
    let old = time("2025-11-01T00:00:00Z");
    let mut ambiguous = raw("bad", 50, 100, "2025-11-01T00:00:00Z");
    ambiguous["chains"] = json!(["日本", "香港", "ChatGPT"]);
    let first = observations(vec![
        ambiguous.clone(),
        raw("active", 1, 2, "2025-11-01T00:00:00Z"),
    ]);
    db.ingest(&first, &topology(), "主机场", old, old)
        .await
        .unwrap();
    ambiguous["upload"] = json!(100);
    db.ingest(
        &observations(vec![ambiguous]),
        &topology(),
        "主机场",
        old,
        old,
    )
    .await
    .unwrap();
    db.ingest(&[], &topology(), "主机场", old, old)
        .await
        .unwrap();
    sqlx::query("UPDATE connections SET status = 'active' WHERE id = 'active'")
        .execute(&db.pool)
        .await
        .unwrap();
    let today = time("2026-11-03T00:00:00Z");
    assert_eq!(retention_cutoff(today).to_string(), "2025-12-01");
    db.cleanup(today).await.unwrap();
    assert_eq!(
        db.connections(&filter("2025-11")).await.unwrap()["total_count"],
        0
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM connections")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let totals = db
        .stats("global", "month", "2025-11-01", "2025-12-01", None)
        .await
        .unwrap();
    assert_eq!(totals[0]["total"], 203);
    let anomalies = db.anomalies(Some("bad"), None, 50, 0).await.unwrap();
    assert_eq!(anomalies["items"][0]["occurrences"], 2);
    assert_eq!(anomalies["items"][0]["first_raw_connection"]["upload"], 50);
    assert_eq!(
        anomalies["items"][0]["latest_raw_connection"]["upload"],
        100
    );
    assert_eq!(
        anomalies["items"][0]["first_raw_connection"]["future_field"]["preserve"],
        true
    );
    assert!(anomalies["items"][0]["raw_context"]["providers"]["providers"].is_object());
    let airports = db
        .stats("airport", "month", "2025-11-01", "2025-12-01", None)
        .await
        .unwrap();
    assert_eq!(
        airports
            .iter()
            .filter(|r| r["route_kind"] == "ambiguous")
            .count(),
        1
    );
}

#[tokio::test]
async fn query_api_auth_validation_pagination_and_removed_routes() {
    let db = Arc::new(Database::open(":memory:").await.unwrap());
    let now = time("2026-11-03T00:00:00Z");
    db.ingest(
        &observations(vec![raw("1", 50, 100, "2026-11-03T00:00:00Z")]),
        &topology(),
        "主机场",
        now,
        now,
    )
    .await
    .unwrap();
    let routes = api::routes(db.clone(), Some("test-token".into()));
    assert_eq!(
        warp::test::request()
            .path("/api/v1/health")
            .reply(&routes)
            .await
            .status(),
        200
    );
    assert_eq!(
        warp::test::request()
            .path("/api/v1/stats")
            .reply(&routes)
            .await
            .status(),
        401
    );
    for path in [
        "/api/v1/stats?dimension=invalid",
        "/api/v1/stats?from=bad",
        "/api/v1/stats?from=2026-12-01&to=2026-11-01",
        "/api/v1/connections?limit=501",
        "/api/v1/connections?month=2026-13",
        "/api/v1/connections?offset=-1",
    ] {
        assert_eq!(
            warp::test::request()
                .path(path)
                .header("authorization", "Bearer test-token")
                .reply(&routes)
                .await
                .status(),
            400,
            "{path}"
        );
    }
    for path in [
        "/api/v1/stats?dimension=airport&period=month&from=2026-11-01&to=2026-12-01",
        "/api/v1/connections?month=2026-11&group=ChatGPT&limit=1",
        "/api/v1/domains?month=2026-11",
        "/api/v1/anomalies",
        "/api/v1/collection-gaps",
    ] {
        let reply = warp::test::request()
            .path(path)
            .header("authorization", "Bearer test-token")
            .reply(&routes)
            .await;
        assert_eq!(
            reply.status(),
            200,
            "{path}: {}",
            String::from_utf8_lossy(reply.body())
        );
        let body: Value = serde_json::from_slice(reply.body()).unwrap();
        assert_eq!(body["status"], "success");
    }
    for path in ["/api/v1/agents", "/api/v1/sync"] {
        assert_eq!(
            warp::test::request()
                .path(path)
                .reply(&routes)
                .await
                .status(),
            404
        );
    }
    db.pool.close().await;
    assert_eq!(
        warp::test::request()
            .path("/api/v1/stats")
            .header("authorization", "Bearer test-token")
            .reply(&routes)
            .await
            .status(),
        500
    );
}

#[tokio::test]
async fn refuses_legacy_database_without_dropping_old_records() {
    let path = std::env::temp_dir().join(format!(
        "mihomo-legacy-{}-{}.db",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("CREATE TABLE connections(id TEXT, agent_id TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO connections VALUES('old', 'agent')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(Database::open(path.to_str().unwrap()).await.is_err());
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT id FROM connections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "old"
    );
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[test]
fn cli_requires_main_airport_and_removes_subcommands() {
    assert!(Config::try_parse_from(["tracker", "--main-airport", "主机场"]).is_ok());
    assert!(Config::try_parse_from(["tracker"]).is_err());
    assert!(Config::try_parse_from(["tracker", "--main-airport", " "]).is_err());
    assert!(Config::try_parse_from(["tracker", "master"]).is_err());
    assert!(Config::try_parse_from(["tracker", "agent"]).is_err());
}

#[tokio::test]
async fn collector_http_websocket_pipeline_and_shutdown() {
    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::{broadcast, watch},
        time::{timeout, Duration},
    };
    use tokio_tungstenite::tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        Message,
    };

    // tungstenite 的握手回调签名固定使用较大的 ErrorResponse。
    #[allow(clippy::result_large_err)]
    fn check_handshake(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        assert_eq!(request.headers()["authorization"], "Bearer mock-secret");
        assert_eq!(request.uri().query(), Some("interval=1000"));
        Ok(response)
    }

    let db = Arc::new(Database::open(":memory:").await.unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (frames, _) = broadcast::channel::<String>(8);
    let server_frames = frames.clone();
    let server = tokio::spawn(async move {
        let mut requests = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (mut stream, _) = accepted.unwrap();
                    let frames = server_frames.clone();
                    requests.spawn(async move {
                        let mut head = [0; 4096];
                        let n = stream.peek(&mut head).await.unwrap();
                        let path = String::from_utf8_lossy(&head[..n]).split_whitespace().nth(1).unwrap().to_owned();
                        if path.starts_with("/connections") {
                            let mut socket = tokio_tungstenite::accept_hdr_async(stream, check_handshake).await.unwrap();
                            let mut receive = frames.subscribe();
                            let mut connection = raw("mock", 100, 200, &Utc::now().to_rfc3339());
                            connection["chains"] = json!(["香港", "ChatGPT"]);
                            connection["providerChains"] = Value::Null;
                            socket.send(Message::Text(json!({"connections": [connection]}).to_string().into())).await.unwrap();
                            loop {
                                tokio::select! {
                                    frame = receive.recv() => {
                                        if socket.send(Message::Text(frame.unwrap().into())).await.is_err() { break; }
                                    }
                                    message = socket.next() => { if message.is_none() { break; } }
                                }
                            }
                        } else {
                            let n = stream.read(&mut head).await.unwrap();
                            assert!(String::from_utf8_lossy(&head[..n]).to_lowercase().contains("authorization: bearer mock-secret"));
                            let context = topology().raw_context;
                            let body = match path.as_str() {
                                "/proxies" => context["proxies"].to_string(),
                                "/providers/proxies" => context["providers"].to_string(),
                                "/version" => context["version"].to_string(),
                                _ => panic!("unexpected endpoint {path}"),
                            };
                            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                            stream.write_all(response.as_bytes()).await.unwrap();
                        }
                    });
                }
                result = requests.join_next(), if !requests.is_empty() => { result.unwrap().unwrap(); }
            }
        }
    });
    let config = Config::try_parse_from([
        "tracker",
        "--main-airport",
        "主机场",
        "--mihomo-port",
        &port.to_string(),
        "--mihomo-token",
        "mock-secret",
    ])
    .unwrap();
    let (stop, receiver) = watch::channel(false);
    let collector_db = db.clone();
    let collector =
        tokio::spawn(async move { crate::collector::run(config, collector_db, receiver).await });
    timeout(Duration::from_secs(5), async {
        while db.health().await.unwrap()["connected"] != true {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut updated = raw("mock", 150, 350, &Utc::now().to_rfc3339());
    updated["chains"] = json!(["香港", "ChatGPT"]);
    updated["providerChains"] = Value::Null;
    frames
        .send(json!({"connections": [updated]}).to_string())
        .unwrap();
    let today = crate::common::local_day(Utc::now());
    timeout(Duration::from_secs(5), async {
        loop {
            let rows = db
                .stats(
                    "airport",
                    "day",
                    &today.to_string(),
                    &today.succ_opt().unwrap().to_string(),
                    Some("主机场"),
                )
                .await
                .unwrap();
            if !rows.is_empty() && rows[0]["total"] == 500 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    timeout(Duration::from_secs(5), collector)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(db.health().await.unwrap()["connected"], false);
    server.abort();
}
