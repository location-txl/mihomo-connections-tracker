use crate::common::{local_day, retention_cutoff, Observation, Result, Topology};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    QueryBuilder, Row, Sqlite, SqlitePool,
};
use std::{collections::HashSet, time::Duration};

pub struct Database {
    pub pool: SqlitePool,
}

pub struct DetailFilter {
    pub from: String,
    pub to: String,
    pub airport: Option<String>,
    pub group: Option<String>,
    pub domain: Option<String>,
    pub rule: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

impl Database {
    pub async fn open(path: &str) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let legacy: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'sync_state')
             OR EXISTS(SELECT 1 FROM pragma_table_info('connections') WHERE name = 'agent_id')",
        )
        .fetch_one(&pool)
        .await?;
        if legacy != 0 {
            pool.close().await;
            return Err(
                "检测到旧主从数据库，请使用新的 --database 路径；旧文件不会迁移或修改".into(),
            );
        }
        // 先识别旧库，再启用 WAL，避免改变旧数据库的持久化设置。
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn begin_gap(&self, at: i64, reason: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO collection_gaps(started_at, reason)
             SELECT COALESCE(last_success_at, ?), ? FROM collector_state
             WHERE id = 1 AND NOT EXISTS(SELECT 1 FROM collection_gaps WHERE ended_at IS NULL)",
        )
        .bind(at)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE collector_state SET connected = 0, last_error = ? WHERE id = 1")
            .bind(reason)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE connections SET status = 'unknown' WHERE status = 'active'")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// 基线、连接日用量及所有统计维度必须一同提交。
    pub async fn ingest(
        &self,
        observations: &[Observation],
        topology: &Topology,
        main_airport: &str,
        observed_at: DateTime<Utc>,
        monitoring_since: DateTime<Utc>,
    ) -> Result<()> {
        let now = observed_at.timestamp_millis();
        let day = local_day(observed_at).to_string();
        let mut tx = self.pool.begin().await?;
        let previous: Vec<String> =
            sqlx::query_scalar("SELECT id FROM connections WHERE status IN ('active', 'unknown')")
                .fetch_all(&mut *tx)
                .await?;
        let current: HashSet<&str> = observations
            .iter()
            .map(|o| o.connection.id.as_str())
            .collect();

        for observation in observations {
            let c = &observation.connection;
            let existing = sqlx::query(
                "SELECT last_upload, last_download, airport, route_kind, attribution_reason
                 FROM connections WHERE id = ?",
            )
            .bind(&c.id)
            .fetch_optional(&mut *tx)
            .await?;
            let (upload, download, airport, route_kind, reason) = if let Some(row) = existing {
                let old_up: i64 = row.try_get("last_upload")?;
                let old_down: i64 = row.try_get("last_download")?;
                let reason: Option<String> = row.try_get("attribution_reason")?;
                let reset = c.upload < old_up || c.download < old_down;
                if reset {
                    Self::anomaly(&mut tx, observation, topology, "counter_reset", now).await?;
                }
                (
                    if reset { 0 } else { c.upload - old_up },
                    if reset { 0 } else { c.download - old_down },
                    row.try_get::<Option<String>, _>("airport")?,
                    row.try_get::<String, _>("route_kind")?,
                    reason,
                )
            } else {
                let attribution = topology.attribute(c, main_airport);
                sqlx::query(
                    "INSERT INTO connections(id, start, first_seen, last_seen, status,
                     last_upload, last_download, airport, route_kind, node, attribution_reason,
                     domain, destination_ip, network, process, rule, rule_payload, metadata, chains, provider_chains)
                     VALUES(?, ?, ?, ?, 'active', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                ).bind(&c.id).bind(c.start.timestamp_millis()).bind(now).bind(now)
                    .bind(c.upload).bind(c.download).bind(&attribution.airport).bind(attribution.route_kind)
                    .bind(&attribution.node).bind(attribution.reason).bind(c.metadata.domain())
                    .bind(&c.metadata.destination_ip).bind(&c.metadata.network).bind(&c.metadata.process)
                    .bind(&c.rule).bind(&c.rule_payload).bind(observation.raw["metadata"].to_string())
                    .bind(serde_json::to_string(&c.chains)?)
                    .bind(serde_json::to_string(&c.provider_chains)?)
                    .execute(&mut *tx).await?;
                for group in &attribution.groups {
                    sqlx::query("INSERT INTO connection_groups(connection_id, name) VALUES(?, ?)")
                        .bind(&c.id)
                        .bind(group)
                        .execute(&mut *tx)
                        .await?;
                }
                let include_initial = c.start >= monitoring_since;
                (
                    if include_initial { c.upload } else { 0 },
                    if include_initial { c.download } else { 0 },
                    attribution.airport,
                    attribution.route_kind.to_owned(),
                    attribution.reason.map(str::to_owned),
                )
            };
            if let Some(reason) = reason {
                Self::anomaly(&mut tx, observation, topology, &reason, now).await?;
            }
            sqlx::query(
                "UPDATE connections SET last_upload = ?, last_download = ?, last_seen = ?,
                 observed_upload = observed_upload + ?, observed_download = observed_download + ?,
                 status = 'active', disappeared_at = NULL, domain = ?, destination_ip = ?, metadata = ?
                 WHERE id = ?"
            ).bind(c.upload).bind(c.download).bind(now).bind(upload).bind(download)
                .bind(c.metadata.domain()).bind(&c.metadata.destination_ip)
                .bind(observation.raw["metadata"].to_string()).bind(&c.id)
                .execute(&mut *tx).await?;
            sqlx::query(
                "INSERT INTO connection_daily_usage(connection_id, day, upload, download) VALUES(?, ?, ?, ?)
                 ON CONFLICT(connection_id, day) DO UPDATE SET
                 upload = upload + excluded.upload, download = download + excluded.download"
            ).bind(&c.id).bind(&day).bind(upload).bind(download).execute(&mut *tx).await?;

            let groups: Vec<String> =
                sqlx::query_scalar("SELECT name FROM connection_groups WHERE connection_id = ?")
                    .bind(&c.id)
                    .fetch_all(&mut *tx)
                    .await?;
            let mut dimensions = vec![
                ("global", String::new()),
                ("airport", airport.unwrap_or_default()),
            ];
            dimensions.extend(groups.into_iter().map(|g| ("group", g)));
            for (dimension, name) in dimensions {
                sqlx::query(
                    "INSERT INTO daily_totals(day, dimension, name, route_kind, upload, download)
                     VALUES(?, ?, ?, ?, ?, ?) ON CONFLICT(day, dimension, name, route_kind)
                     DO UPDATE SET upload = upload + excluded.upload, download = download + excluded.download"
                ).bind(&day).bind(dimension).bind(name).bind(&route_kind).bind(upload).bind(download)
                    .execute(&mut *tx).await?;
            }
        }
        for id in previous {
            if !current.contains(id.as_str()) {
                sqlx::query("UPDATE connections SET status = 'disappeared', disappeared_at = ? WHERE id = ?")
                    .bind(now).bind(id).execute(&mut *tx).await?;
            }
        }
        sqlx::query("UPDATE collection_gaps SET ended_at = ? WHERE ended_at IS NULL")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE collector_state SET connected = 1, last_success_at = ?, last_error = NULL WHERE id = 1")
            .bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn anomaly(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        observation: &Observation,
        topology: &Topology,
        reason: &str,
        now: i64,
    ) -> Result<()> {
        let raw = observation.raw.to_string();
        sqlx::query(
            "INSERT INTO attribution_anomalies(connection_id, reason, first_seen, last_seen,
             occurrences, first_raw_connection, latest_raw_connection, raw_context)
             VALUES(?, ?, ?, ?, 1, ?, ?, ?) ON CONFLICT(connection_id, reason) DO UPDATE SET
             last_seen = excluded.last_seen, occurrences = occurrences + 1,
             latest_raw_connection = excluded.latest_raw_connection",
        )
        .bind(&observation.connection.id)
        .bind(reason)
        .bind(now)
        .bind(now)
        .bind(&raw)
        .bind(&raw)
        .bind(topology.raw_context.to_string())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn cleanup(&self, now: DateTime<Utc>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM connection_daily_usage WHERE day < ?")
            .bind(retention_cutoff(now).to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM connections WHERE status = 'disappeared'
             AND NOT EXISTS(SELECT 1 FROM connection_daily_usage WHERE connection_id = connections.id)"
        ).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn health(&self) -> Result<Value> {
        let row = sqlx::query(
            "SELECT connected, last_success_at, last_error FROM collector_state WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(json!({"connected": row.try_get::<bool, _>("connected")?,
            "last_success_at": row.try_get::<Option<i64>, _>("last_success_at")?,
            "last_error": row.try_get::<Option<String>, _>("last_error")?,
            "timezone": "Asia/Shanghai", "sampling_interval_ms": 1000}))
    }

    pub async fn stats(
        &self,
        dimension: &str,
        period: &str,
        from: &str,
        to: &str,
        name: Option<&str>,
    ) -> Result<Vec<Value>> {
        let period_expr = if period == "month" {
            "substr(day, 1, 7)"
        } else {
            "day"
        };
        let mut query = QueryBuilder::<Sqlite>::new("SELECT json_object('period', ");
        query.push(period_expr).push(", 'name', ");
        query.push(if dimension == "global" {
            "NULL"
        } else {
            "NULLIF(name, '')"
        });
        query
            .push(", 'route_kind', ")
            .push(if dimension == "airport" {
                "route_kind"
            } else {
                "NULL"
            });
        query.push(", 'upload', SUM(upload), 'download', SUM(download), 'total', SUM(upload) + SUM(download)) AS item
            FROM daily_totals WHERE dimension = ").push_bind(dimension.to_owned());
        query
            .push(" AND day >= ")
            .push_bind(from.to_owned())
            .push(" AND day < ")
            .push_bind(to.to_owned());
        if let Some(name) = name {
            query.push(" AND name = ").push_bind(name.to_owned());
        }
        query.push(" GROUP BY ").push(period_expr);
        if dimension != "global" {
            query.push(", name");
        }
        if dimension == "airport" {
            query.push(", route_kind");
        }
        query.push(" ORDER BY ").push(period_expr);
        if dimension != "global" {
            query.push(", name");
        }
        self.json_rows(&mut query).await
    }

    fn detail_query(select: &str, filter: &DetailFilter) -> QueryBuilder<Sqlite> {
        let mut query = QueryBuilder::new(
            "WITH usage AS (
            SELECT connection_id, SUM(upload) AS upload, SUM(download) AS download
            FROM connection_daily_usage WHERE day >= ",
        );
        query
            .push_bind(filter.from.clone())
            .push(" AND day < ")
            .push_bind(filter.to.clone());
        query.push(" GROUP BY connection_id) SELECT ").push(select);
        query.push(" FROM usage u JOIN connections c ON c.id = u.connection_id WHERE 1 = 1");
        for (column, value) in [
            ("c.airport", &filter.airport),
            ("c.domain", &filter.domain),
            ("c.rule_payload", &filter.rule),
        ] {
            if let Some(value) = value {
                query
                    .push(" AND ")
                    .push(column)
                    .push(" = ")
                    .push_bind(value.clone());
            }
        }
        if let Some(group) = &filter.group {
            query.push(" AND EXISTS(SELECT 1 FROM connection_groups g WHERE g.connection_id = c.id AND g.name = ")
                .push_bind(group.clone()).push(")");
        }
        query
    }

    pub async fn connections(&self, filter: &DetailFilter) -> Result<Value> {
        let total: i64 = Self::detail_query("COUNT(*)", filter)
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;
        let mut query = Self::detail_query(
            "json_object('id', c.id, 'start', c.start, 'first_seen', c.first_seen,
             'last_seen', c.last_seen, 'disappeared_at', c.disappeared_at, 'status', c.status,
             'airport', c.airport, 'route_kind', c.route_kind, 'node', c.node,
             'domain', c.domain, 'destination_ip', c.destination_ip, 'network', c.network,
             'process', c.process, 'rule', c.rule, 'rule_payload', c.rule_payload,
             'metadata', json(c.metadata), 'chains', json(c.chains), 'provider_chains', json(c.provider_chains),
             'groups', json((SELECT json_group_array(name) FROM connection_groups WHERE connection_id = c.id)),
             'upload', u.upload, 'download', u.download, 'total', u.upload + u.download,
             'observed_upload', c.observed_upload, 'observed_download', c.observed_download,
             'last_raw_upload', c.last_upload, 'last_raw_download', c.last_download) AS item", filter);
        query
            .push(" ORDER BY c.first_seen DESC, c.id LIMIT ")
            .push_bind(filter.limit)
            .push(" OFFSET ")
            .push_bind(filter.offset);
        Ok(
            json!({"items": self.json_rows(&mut query).await?, "total_count": total,
            "limit": filter.limit, "offset": filter.offset, "from": filter.from, "to": filter.to}),
        )
    }

    pub async fn domains(&self, filter: &DetailFilter) -> Result<Value> {
        let mut query = Self::detail_query(
            "json_object('domain', c.domain, 'connection_count', COUNT(*), 'upload', SUM(u.upload),
             'download', SUM(u.download), 'total', SUM(u.upload) + SUM(u.download)) AS item",
            filter,
        );
        query
            .push(
                " GROUP BY c.domain ORDER BY SUM(u.upload) + SUM(u.download) DESC, c.domain LIMIT ",
            )
            .push_bind(filter.limit)
            .push(" OFFSET ")
            .push_bind(filter.offset);
        Ok(
            json!({"items": self.json_rows(&mut query).await?, "limit": filter.limit, "offset": filter.offset,
            "from": filter.from, "to": filter.to}),
        )
    }

    pub async fn anomalies(
        &self,
        connection: Option<&str>,
        reason: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Value> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT json_object('id', id, 'connection_id', connection_id, 'reason', reason,
             'first_seen', first_seen, 'last_seen', last_seen, 'occurrences', occurrences,
             'first_raw_connection', json(first_raw_connection), 'latest_raw_connection', json(latest_raw_connection),
             'raw_context', json(raw_context)) AS item FROM attribution_anomalies WHERE 1 = 1");
        for (column, value) in [("connection_id", connection), ("reason", reason)] {
            if let Some(value) = value {
                query
                    .push(" AND ")
                    .push(column)
                    .push(" = ")
                    .push_bind(value.to_owned());
            }
        }
        query
            .push(" ORDER BY last_seen DESC, id DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        Ok(json!({"items": self.json_rows(&mut query).await?, "limit": limit, "offset": offset}))
    }

    pub async fn gaps(&self, from: i64, to: i64) -> Result<Vec<Value>> {
        let mut query = QueryBuilder::new(
            "SELECT json_object('id', id, 'started_at', started_at, 'ended_at', ended_at, 'reason', reason) AS item
             FROM collection_gaps WHERE started_at < ");
        query
            .push_bind(to)
            .push(" AND (ended_at IS NULL OR ended_at > ")
            .push_bind(from)
            .push(") ORDER BY started_at, id");
        self.json_rows(&mut query).await
    }

    async fn json_rows(&self, query: &mut QueryBuilder<Sqlite>) -> Result<Vec<Value>> {
        let rows = query.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(serde_json::from_str::<Value>(
                    &row.try_get::<String, _>("item")?,
                )?)
            })
            .collect()
    }
}
