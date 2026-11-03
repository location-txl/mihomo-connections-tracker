use crate::{
    common::{parse_snapshot, Result, Topology},
    config::Config,
    db::Database,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::watch,
    time::{Instant, MissedTickBehavior},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{header::AUTHORIZATION, HeaderValue},
        Message,
    },
};

async fn topology(client: &Client, base: &str, token: Option<&str>) -> Result<Topology> {
    async fn get(client: &Client, url: String, token: Option<&str>) -> Result<Value> {
        let mut request = client.get(url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        Ok(request.send().await?.error_for_status()?.json().await?)
    }
    let (proxies, providers, version) = tokio::try_join!(
        get(client, format!("{base}/proxies"), token),
        get(client, format!("{base}/providers/proxies"), token),
        get(client, format!("{base}/version"), token),
    )?;
    Topology::parse(proxies, providers, version, Utc::now().timestamp_millis())
}

pub async fn run(
    config: Config,
    db: Arc<Database>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let monitoring_since = Utc::now();
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?;
    let host = if config.mihomo_host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{}]", config.mihomo_host)
    } else {
        config.mihomo_host.clone()
    };
    let base = format!("http://{host}:{}", config.mihomo_port);
    let ws_url = format!(
        "ws://{host}:{}/connections?interval=1000",
        config.mihomo_port
    );
    let mut request = ws_url.into_client_request()?;
    if let Some(token) = &config.mihomo_token {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))?;
        value.set_sensitive(true);
        request.headers_mut().insert(AUTHORIZATION, value);
    }
    db.begin_gap(monitoring_since.timestamp_millis(), "service_start")
        .await?;
    db.cleanup(Utc::now()).await?;
    let mut cleaned_day = crate::common::local_day(Utc::now());

    loop {
        if *shutdown.borrow() {
            break;
        }
        let topology_result = tokio::select! {
            result = topology(&client, &base, config.mihomo_token.as_deref()) => result,
            _ = shutdown.changed() => break,
        };
        match topology_result {
            Ok(mut context) => {
                let connection = tokio::select! {
                    result = tokio::time::timeout(Duration::from_secs(8), connect_async(request.clone())) => result,
                    _ = shutdown.changed() => break,
                };
                match connection {
                    Ok(Ok((mut socket, _))) => {
                        println!("Mihomo 已连接，等待连接快照");
                        let mut refresh = tokio::time::interval_at(
                            Instant::now() + Duration::from_secs(60),
                            Duration::from_secs(60),
                        );
                        refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);
                        let mut last_snapshot = Instant::now();
                        loop {
                            tokio::select! {
                                _ = shutdown.changed() => break,
                                _ = tokio::time::sleep_until(last_snapshot + Duration::from_secs(15)) => {
                                    eprintln!("Mihomo 15 秒未返回有效快照，重新连接");
                                    db.begin_gap(Utc::now().timestamp_millis(), "snapshot_timeout").await?;
                                    break;
                                }
                                _ = refresh.tick() => {
                                    match topology(&client, &base, config.mihomo_token.as_deref()).await {
                                        Ok(updated) => context = updated,
                                        Err(error) => eprintln!("刷新代理信息失败，继续使用已保存的拓扑：{error}"),
                                    }
                                }
                                message = socket.next() => {
                                    match message {
                                        Some(Ok(Message::Text(text))) => {
                                            match parse_snapshot(&text) {
                                                Ok(observations) => {
                                                    let now = Utc::now();
                                                    db.ingest(&observations, &context, &config.main_airport, now, monitoring_since).await?;
                                                    last_snapshot = Instant::now();
                                                    let today = crate::common::local_day(now);
                                                    if today != cleaned_day {
                                                        db.cleanup(now).await?;
                                                        cleaned_day = today;
                                                    }
                                                }
                                                Err(error) => {
                                                    eprintln!("连接快照无效，未推进计数基线：{error}");
                                                    db.begin_gap(Utc::now().timestamp_millis(), "invalid_snapshot").await?;
                                                    break;
                                                }
                                            }
                                        }
                                        Some(Ok(Message::Ping(data))) => {
                                            if socket.send(Message::Pong(data)).await.is_err() {
                                                db.begin_gap(Utc::now().timestamp_millis(), "websocket_disconnected").await?;
                                                break;
                                            }
                                        }
                                        Some(Ok(Message::Close(_))) | None => {
                                            db.begin_gap(Utc::now().timestamp_millis(), "websocket_disconnected").await?;
                                            break;
                                        }
                                        Some(Err(error)) => {
                                            eprintln!("Mihomo WebSocket 断开：{error}");
                                            db.begin_gap(Utc::now().timestamp_millis(), "websocket_disconnected").await?;
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        eprintln!("连接 Mihomo 失败：{error}");
                        db.begin_gap(Utc::now().timestamp_millis(), "connection_failed")
                            .await?;
                    }
                    Err(_) => {
                        eprintln!("连接 Mihomo 超时");
                        db.begin_gap(Utc::now().timestamp_millis(), "connection_timeout")
                            .await?;
                    }
                }
            }
            Err(error) => {
                eprintln!("读取 Mihomo 代理信息失败：{error}");
                db.begin_gap(Utc::now().timestamp_millis(), "topology_unavailable")
                    .await?;
            }
        }
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
    db.begin_gap(Utc::now().timestamp_millis(), "service_stopped")
        .await?;
    Ok(())
}
