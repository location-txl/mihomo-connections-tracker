use chrono::{DateTime, Datelike, FixedOffset, Months, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn local_day(time: DateTime<Utc>) -> NaiveDate {
    time.with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
        .date_naive()
}

/// 未指定 --database 时使用的服务数据库路径。
pub fn default_database_path() -> Result<String> {
    let home = std::env::var_os("HOME").ok_or("未设置 HOME")?;
    Ok(std::path::Path::new(&home)
        .join("Library/Application Support/mihomo-connections-tracker/tracker.db")
        .display()
        .to_string())
}

pub fn month_start(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap()
}

pub fn retention_cutoff(now: DateTime<Utc>) -> NaiveDate {
    month_start(local_day(now)) - Months::new(11)
}

#[derive(Debug, Deserialize)]
pub struct Connection {
    pub id: String,
    pub upload: i64,
    pub download: i64,
    pub start: DateTime<Utc>,
    pub metadata: Metadata,
    pub chains: Vec<String>,
    #[serde(rename = "providerChains", default)]
    pub provider_chains: Option<Vec<Option<String>>>,
    pub rule: String,
    #[serde(rename = "rulePayload")]
    pub rule_payload: String,
}

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub network: String,
    #[serde(default)]
    pub host: String,
    #[serde(rename = "sniffHost", default)]
    pub sniff_host: String,
    #[serde(rename = "destinationIP", default)]
    pub destination_ip: String,
    #[serde(default)]
    pub process: String,
}

impl Metadata {
    pub fn domain(&self) -> Option<String> {
        let host = if self.host.is_empty() {
            &self.sniff_host
        } else {
            &self.host
        };
        if host.is_empty() || host.parse::<std::net::IpAddr>().is_ok() {
            None
        } else {
            Some(host.trim_end_matches('.').to_lowercase())
        }
    }
}

pub struct Observation {
    pub connection: Connection,
    pub raw: Value,
}

pub fn parse_snapshot(text: &str) -> Result<Vec<Observation>> {
    #[derive(Deserialize)]
    struct Snapshot {
        #[serde(deserialize_with = "nullable_connections")]
        connections: Vec<Value>,
    }
    fn nullable_connections<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<Value>, D::Error> {
        Ok(Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default())
    }

    let snapshot: Snapshot = serde_json::from_str(text)?;
    let mut ids = HashSet::new();
    let mut observations = Vec::with_capacity(snapshot.connections.len());
    for raw in snapshot.connections {
        let connection: Connection = serde_json::from_value(raw.clone())?;
        if connection.id.is_empty()
            || connection.upload < 0
            || connection.download < 0
            || connection.upload.checked_add(connection.download).is_none()
            || !ids.insert(connection.id.clone())
        {
            return Err("连接 ID 或字节计数无效".into());
        }
        observations.push(Observation { connection, raw });
    }
    Ok(observations)
}

#[derive(Clone, Deserialize)]
pub struct Proxy {
    #[serde(rename = "type")]
    pub kind: String,
    pub all: Option<Vec<String>>,
    #[serde(rename = "dialer-proxy", default)]
    pub dialer_proxy: String,
}

pub struct Topology {
    pub proxies: HashMap<String, Proxy>,
    pub providers: HashMap<String, HashMap<String, Proxy>>,
    pub raw_context: Value,
}

impl Topology {
    pub fn parse(
        proxies: Value,
        providers: Value,
        version: Value,
        fetched_at: i64,
    ) -> Result<Self> {
        #[derive(Deserialize)]
        struct Proxies {
            proxies: HashMap<String, Proxy>,
        }
        #[derive(Deserialize)]
        struct ProviderProxy {
            name: String,
            #[serde(flatten)]
            proxy: Proxy,
        }
        #[derive(Deserialize)]
        struct Provider {
            proxies: Vec<ProviderProxy>,
        }
        #[derive(Deserialize)]
        struct Providers {
            providers: HashMap<String, Provider>,
        }
        let parsed_proxies: Proxies = serde_json::from_value(proxies.clone())?;
        let parsed_providers: Providers = serde_json::from_value(providers.clone())?;
        Ok(Self {
            proxies: parsed_proxies.proxies,
            providers: parsed_providers
                .providers
                .into_iter()
                .map(|(name, provider)| {
                    (
                        name,
                        provider
                            .proxies
                            .into_iter()
                            .map(|p| (p.name, p.proxy))
                            .collect(),
                    )
                })
                .collect(),
            raw_context: serde_json::json!({
                "fetched_at": fetched_at, "version": version,
                "proxies": proxies, "providers": providers
            }),
        })
    }

    pub fn attribute(&self, conn: &Connection, main_airport: &str) -> Attribution {
        let mut groups = Vec::new();
        let mut terminals = Vec::new();
        for (index, name) in conn.chains.iter().enumerate() {
            if self.proxies.get(name).is_some_and(|p| p.all.is_some()) {
                if !groups.contains(name) {
                    groups.push(name.clone());
                }
            } else {
                terminals.push((index, name));
            }
        }
        let mut result = Attribution {
            airport: None,
            route_kind: "ambiguous",
            node: None,
            groups,
            reason: None,
        };
        if conn
            .provider_chains
            .as_ref()
            .is_some_and(|p| p.len() != conn.chains.len())
        {
            result.reason = Some("chain_length_mismatch");
            return result;
        }
        if terminals.len() != 1 {
            result.reason = Some(if terminals.is_empty() {
                "missing_terminal"
            } else {
                "ambiguous_multihop"
            });
            return result;
        }
        let (index, name) = terminals[0];
        result.node = Some(name.clone());
        let provider = conn
            .provider_chains
            .as_ref()
            .and_then(|p| p[index].as_deref())
            .filter(|p| !p.is_empty());
        let proxy = match provider {
            Some(provider) => self.providers.get(provider).and_then(|p| p.get(name)),
            None => self.proxies.get(name),
        };
        let Some(proxy) = proxy else {
            result.reason = Some("unresolved_node");
            return result;
        };
        if !proxy.dialer_proxy.is_empty() {
            result.reason = Some("ambiguous_multihop");
            return result;
        }
        match proxy.kind.as_str() {
            "Direct" | "DIRECT" => result.route_kind = "direct",
            "Reject" | "REJECT" | "RejectDrop" => result.route_kind = "reject",
            "Pass" | "PASS" => result.reason = Some("unresolved_terminal"),
            _ => {
                result.route_kind = "proxy";
                result.airport = Some(provider.unwrap_or(main_airport).to_owned());
            }
        }
        result
    }
}

pub struct Attribution {
    pub airport: Option<String>,
    pub route_kind: &'static str,
    pub node: Option<String>,
    pub groups: Vec<String>,
    pub reason: Option<&'static str>,
}
