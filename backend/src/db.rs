use crate::common::ConnectionRecord;
use sqlx::{
    AssertSqlSafe, Error as SqlxError, Executor, Sqlite, SqlitePool, query as sqlx_query,
    sqlite::SqliteConnectOptions, Row, Column,
};
use std::collections::HashMap;
use serde_json::{Value as JsonValue, Number as JsonNumber};
use chrono::Utc;

// 查询结果类型
pub type QueryResult<T> = Result<T, SqlxError>;
pub type JsonResult = QueryResult<Vec<JsonValue>>;

// 数据库连接池
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    // 初始化数据库
    pub async fn new(database_path: &str) -> Result<Self, SqlxError> {
        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;

        // 创建连接表
        Self::init_db_schema(&pool).await?;
        Ok(Self { pool })
    }
    
    // 初始化数据库架构
    async fn init_db_schema(pool: &SqlitePool) -> Result<(), SqlxError> {
        // 创建连接表
        pool.execute(
            r#"
            CREATE TABLE IF NOT EXISTS connections (
                id TEXT,
                conn_download INTEGER,
                conn_upload INTEGER,
                last_updated TEXT,
                start TEXT,
                network TEXT,
                type TEXT,
                source_ip TEXT,
                destination_ip TEXT,
                source_geoip TEXT,
                destination_geoip TEXT,
                source_ip_asn TEXT,
                destination_ip_asn TEXT,
                source_port TEXT,
                destination_port TEXT,
                inbound_ip TEXT,
                inbound_port TEXT,
                inbound_name TEXT,
                inbound_user TEXT,
                host TEXT,
                dns_mode TEXT,
                uid INTEGER,
                process TEXT,
                process_path TEXT,
                special_proxy TEXT,
                special_rules TEXT,
                remote_destination TEXT,
                dscp INTEGER,
                sniff_host TEXT,
                chains TEXT,
                rule TEXT,
                rule_payload TEXT,
                agent_id TEXT,
                PRIMARY KEY (id, agent_id)
            )
            "#,
        ).await?;

        // 创建同步状态表（用于从机）
        pool.execute(
            r#"
            CREATE TABLE IF NOT EXISTS sync_state (
                id INTEGER PRIMARY KEY,
                last_sync_time TEXT,
                pending_records INTEGER,
                last_connection_id TEXT
            )
            "#,
        ).await?;

        // 初始化同步状态（如果尚未有记录）
        pool.execute(
            r#"
            INSERT OR IGNORE INTO sync_state (id, last_sync_time, pending_records, last_connection_id)
            VALUES (1, datetime('now', 'utc'), 0, '')
            "#,
        ).await?;
        
        Ok(())
    }

    // 通用的参数绑定方法
    fn bind_params<'a>(query: sqlx::query::Query<'a, Sqlite, sqlx::sqlite::SqliteArguments>, params: &'a [String]) -> sqlx::query::Query<'a, Sqlite, sqlx::sqlite::SqliteArguments> {
        let mut bound_query = query;
        for param in params {
            bound_query = bound_query.bind(param);
        }
        bound_query
    }
    
    // 通用查询执行方法
    async fn execute_query<F, T>(&self, sql: &str, params: &[String], row_mapper: F) -> QueryResult<Vec<T>>
    where
        F: FnMut(sqlx::sqlite::SqliteRow) -> T,
    {
        let query = sqlx::query(AssertSqlSafe(sql));
        let query = Self::bind_params(query, params);
        let rows = query.fetch_all(&self.pool).await?;
        
        Ok(rows.into_iter().map(row_mapper).collect())
    }
    
    // ==================== 同步相关方法 ====================

    // 查询尚未同步的记录数量
    pub async fn count_pending_records(&self) -> QueryResult<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM connections WHERE last_updated > (SELECT last_sync_time FROM sync_state WHERE id = 1)")
            .fetch_one(&self.pool)
            .await?;
            
        let count: i64 = row.get(0);
        Ok(count)
    }

    // 更新同步状态
    pub async fn update_sync_state(&self, time: &str, pending: i64) -> QueryResult<()> {
        sqlx::query("UPDATE sync_state SET last_sync_time = ?, pending_records = ? WHERE id = 1")
            .bind(time)
            .bind(pending)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // 从数据库中获取待同步的记录
    pub async fn get_pending_records(&self, limit: i64) -> QueryResult<Vec<ConnectionRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                id, conn_download, conn_upload, last_updated, start, 
                network, type as conn_type, source_ip, destination_ip, 
                source_geoip, destination_geoip, source_ip_asn, destination_ip_asn, 
                source_port, destination_port, inbound_ip, inbound_port, 
                inbound_name, inbound_user, host, dns_mode, uid, process, 
                process_path, special_proxy, special_rules, remote_destination, 
                dscp, sniff_host, chains, rule, rule_payload, agent_id
            FROM connections
            WHERE last_updated > (SELECT last_sync_time FROM sync_state WHERE id = 1)
            ORDER BY last_updated ASC
            LIMIT ?
            "#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(ConnectionRecord {
                id: row.get("id"),
                download: row.get("conn_download"),
                upload: row.get("conn_upload"),
                last_updated: row.get("last_updated"),
                start: row.get("start"),
                network: row.get("network"),
                conn_type: row.get("conn_type"),
                source_ip: row.get("source_ip"),
                destination_ip: row.get("destination_ip"),
                source_geoip: row.get("source_geoip"),
                destination_geoip: row.get("destination_geoip"),
                source_ip_asn: row.get("source_ip_asn"),
                destination_ip_asn: row.get("destination_ip_asn"),
                source_port: row.get("source_port"),
                destination_port: row.get("destination_port"),
                inbound_ip: row.get("inbound_ip"),
                inbound_port: row.get("inbound_port"),
                inbound_name: row.get("inbound_name"),
                inbound_user: row.get("inbound_user"),
                host: row.get("host"),
                dns_mode: row.get("dns_mode"),
                uid: row.get("uid"),
                process: row.get("process"),
                process_path: row.get("process_path"),
                special_proxy: row.get("special_proxy"),
                special_rules: row.get("special_rules"),
                remote_destination: row.get("remote_destination"),
                dscp: row.get("dscp"),
                sniff_host: row.get("sniff_host"),
                chains: row.get("chains"),
                rule: row.get("rule"),
                rule_payload: row.get("rule_payload"),
                agent_id: row.get("agent_id"),
            });
        }

        Ok(records)
    }

    // 将连接记录保存到数据库中
    pub async fn upsert_connection_record<'a, E>(&self, exec: E, record: &ConnectionRecord) -> QueryResult<()>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx_query(
            r#"
            INSERT INTO connections (
                id, conn_download, conn_upload, last_updated, start, network, type,
                source_ip, destination_ip, source_geoip, destination_geoip, source_ip_asn,
                destination_ip_asn, source_port, destination_port, inbound_ip, inbound_port,
                inbound_name, inbound_user, host, dns_mode, uid, process, process_path,
                special_proxy, special_rules, remote_destination, dscp, sniff_host, chains,
                rule, rule_payload, agent_id
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id, agent_id) DO UPDATE SET
                conn_download=excluded.conn_download,
                conn_upload=excluded.conn_upload,
                last_updated=excluded.last_updated,
                start=excluded.start,
                network=excluded.network,
                type=excluded.type,
                source_ip=excluded.source_ip,
                destination_ip=excluded.destination_ip,
                source_geoip=excluded.source_geoip,
                destination_geoip=excluded.destination_geoip,
                source_ip_asn=excluded.source_ip_asn,
                destination_ip_asn=excluded.destination_ip_asn,
                source_port=excluded.source_port,
                destination_port=excluded.destination_port,
                inbound_ip=excluded.inbound_ip,
                inbound_port=excluded.inbound_port,
                inbound_name=excluded.inbound_name,
                inbound_user=excluded.inbound_user,
                host=excluded.host,
                dns_mode=excluded.dns_mode,
                uid=excluded.uid,
                process=excluded.process,
                process_path=excluded.process_path,
                special_proxy=excluded.special_proxy,
                special_rules=excluded.special_rules,
                remote_destination=excluded.remote_destination,
                dscp=excluded.dscp,
                sniff_host=excluded.sniff_host,
                chains=excluded.chains,
                rule=excluded.rule,
                rule_payload=excluded.rule_payload
            "#,
        )
        .bind(&record.id)
        .bind(record.download)
        .bind(record.upload)
        .bind(&record.last_updated)
        .bind(&record.start)
        .bind(&record.network)
        .bind(&record.conn_type)
        .bind(&record.source_ip)
        .bind(&record.destination_ip)
        .bind(&record.source_geoip)
        .bind(&record.destination_geoip)
        .bind(&record.source_ip_asn)
        .bind(&record.destination_ip_asn)
        .bind(&record.source_port)
        .bind(&record.destination_port)
        .bind(&record.inbound_ip)
        .bind(&record.inbound_port)
        .bind(&record.inbound_name)
        .bind(&record.inbound_user)
        .bind(&record.host)
        .bind(&record.dns_mode)
        .bind(record.uid)
        .bind(&record.process)
        .bind(&record.process_path)
        .bind(&record.special_proxy)
        .bind(&record.special_rules)
        .bind(&record.remote_destination)
        .bind(record.dscp)
        .bind(&record.sniff_host)
        .bind(&record.chains)
        .bind(&record.rule)
        .bind(&record.rule_payload)
        .bind(&record.agent_id)
        .execute(exec)
        .await?;
        Ok(())
    }

    // 批量插入连接记录
    pub async fn batch_upsert_records(&self, records: &[ConnectionRecord]) -> QueryResult<()> {
        let mut tx = self.pool.begin().await?;
        for record in records {
            self.upsert_connection_record(&mut *tx, record).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    
    // ==================== 统计查询方法 ====================
    
    // 执行计数查询
    pub async fn execute_count_query(&self, sql: &str, params: &[String]) -> QueryResult<i64> {
        let query = sqlx::query(AssertSqlSafe(sql));
        let query = Self::bind_params(query, params);
        let row = query.fetch_one(&self.pool).await?;
        let count: i64 = row.get(0);
        Ok(count)
    }
    
    // 执行流量查询
    pub async fn execute_traffic_query(&self, sql: &str, params: &[String]) -> QueryResult<(i64, i64)> {
        let query = sqlx::query(AssertSqlSafe(sql));
        let query = Self::bind_params(query, params);
        let row = query.fetch_one(&self.pool).await?;
        
        // 获取下载和上传流量，如果为空则返回0
        let download: Option<i64> = row.try_get(0).unwrap_or(Some(0));
        let upload: Option<i64> = row.try_get(1).unwrap_or(Some(0));
        
        Ok((download.unwrap_or(0), upload.unwrap_or(0)))
    }
    
    // 从SQL列名中提取显示字段名
    fn extract_field_name(col_name: &str) -> String {
        if col_name.to_lowercase().contains(" as ") {
            // 如果有AS关键字，提取别名
            col_name.to_lowercase()
                .split(" as ")
                .nth(1)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| col_name.to_string())
        } else {
            col_name.to_string()
        }
    }
    
    // 安全获取行的值
    fn get_row_value(row: &sqlx::sqlite::SqliteRow, index: usize) -> JsonValue {
        match row.try_get::<String, _>(index) {
            Ok(val) => JsonValue::String(val),
            Err(_) => match row.try_get::<i64, _>(index) {
                Ok(val) => JsonValue::Number(JsonNumber::from(val)),
                Err(_) => match row.try_get::<f64, _>(index) {
                    Ok(val) => JsonValue::from(val),
                    Err(_) => JsonValue::Null,
                },
            },
        }
    }
    
    // 执行分组查询
    pub async fn execute_group_query(&self, sql: &str, params: &[String]) -> JsonResult {
        // 执行查询
        let rows = self.execute_query(sql, params, |row| row).await?;
        
        // 转换结果为JSON数组
        let mut results = Vec::with_capacity(rows.len());
        
        for row in rows {
            // 灵活处理不同类型的分组字段
            let mut result_obj = serde_json::Map::new();
            
            // 处理规则和rule_payload的情况 - 这时row有两个列作为分组键
            if sql.contains("GROUP BY rule, rule_payload") {
                // 处理规则组合
                self.process_rule_group(&row, &mut result_obj);
            } else {
                // 标准的单列分组情况
                self.process_standard_group(&row, &mut result_obj, sql);
            }
            
            // 添加统计计数
            self.add_count_metrics(&row, &mut result_obj);
            
            results.push(JsonValue::Object(result_obj));
        }
        
        Ok(results)
    }
    
    // 处理规则分组结果
    fn process_rule_group(&self, row: &sqlx::sqlite::SqliteRow, result_obj: &mut serde_json::Map<String, JsonValue>) {
                if let Ok(rule) = row.try_get::<String, _>(0) {
            result_obj.insert("rule".to_string(), JsonValue::String(rule));
                }
        
                if let Ok(payload) = row.try_get::<String, _>(1) {
            // 创建组合规则值
            if let (Some(rule_val), true) = (result_obj.get("rule"), !payload.is_empty()) {
                if let Some(rule_str) = rule_val.as_str() {
                    let combined = format!("{} ({})", rule_str, payload);
                    result_obj.insert("rule".to_string(), JsonValue::String(combined));
                }
            } else {
                result_obj.insert("rule_payload".to_string(), JsonValue::String(payload));
            }
                }
                
                // 此时count, download, upload的列索引会右移
                if let Ok(count) = row.try_get::<i64, _>(2) {
            result_obj.insert("count".to_string(), JsonValue::Number(count.into()));
                }
                if let Ok(download) = row.try_get::<i64, _>(3) {
            result_obj.insert("download".to_string(), JsonValue::Number(download.into()));
                }
                if let Ok(upload) = row.try_get::<i64, _>(4) {
            result_obj.insert("upload".to_string(), JsonValue::Number(upload.into()));
                }
                
                // 计算总流量
                let download = row.try_get::<i64, _>(3).unwrap_or(0);
                let upload = row.try_get::<i64, _>(4).unwrap_or(0);
        result_obj.insert("total".to_string(), JsonValue::Number((download + upload).into()));
    }
    
    // 处理标准分组结果
    fn process_standard_group(&self, row: &sqlx::sqlite::SqliteRow, result_obj: &mut serde_json::Map<String, JsonValue>, sql: &str) {
        // 获取分组键
        let group_key = Self::get_row_value(row, 0);
                
                // 提取列名并设置为键
                if let Some(col_name) = sql.split("SELECT ").nth(1).and_then(|s| s.split(',').next()) {
                    let col_name = col_name.trim();
            let key_name = Self::extract_field_name(col_name);
            result_obj.insert(key_name, group_key);
        }
    }
    
    // 添加统计指标
    fn add_count_metrics(&self, row: &sqlx::sqlite::SqliteRow, result_obj: &mut serde_json::Map<String, JsonValue>) {
        // 如果已经添加了count字段(在规则组合处理中)，则跳过
        if result_obj.contains_key("count") {
            return;
                }
                
                // 标准统计列
                if let Ok(count) = row.try_get::<i64, _>(1) {
            result_obj.insert("count".to_string(), JsonValue::Number(count.into()));
                }
                if let Ok(download) = row.try_get::<i64, _>(2) {
            result_obj.insert("download".to_string(), JsonValue::Number(download.into()));
                }
                if let Ok(upload) = row.try_get::<i64, _>(3) {
            result_obj.insert("upload".to_string(), JsonValue::Number(upload.into()));
                }
                
                // 计算总流量
                let download = row.try_get::<i64, _>(2).unwrap_or(0);
                let upload = row.try_get::<i64, _>(3).unwrap_or(0);
        result_obj.insert("total".to_string(), JsonValue::Number((download + upload).into()));
    }
    
    // 处理chains分组结果
    pub async fn process_chains_results(&self, results: Vec<JsonValue>, sort_by: Option<&str>, sort_order: Option<&str>, limit: Option<u32>) -> JsonResult {
        let mut processed_results = Vec::new();
        
        for result in results {
            if let Some(obj) = result.as_object() {
                if let Some(chains_value) = obj.get("chains") {
                    if let Some(chains_str) = chains_value.as_str() {
                        // 解析chains字符串
                        let mut node = chains_str.to_string();
                        
                        // 尝试解析JSON数组
                        let clean_chains = chains_str.trim().replace('\'', "\"");
                        if let Ok(array) = serde_json::from_str::<Vec<String>>(&clean_chains) {
                            if !array.is_empty() {
                                node = array.last().unwrap().clone();
                            }
                        }
                        
                        // 创建节点结果
                        let count = obj.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                        let download = obj.get("download").and_then(|v| v.as_i64()).unwrap_or(0);
                        let upload = obj.get("upload").and_then(|v| v.as_i64()).unwrap_or(0);
                        
                        // 将结果聚合到节点统计中
                        self.aggregate_node_stats(&mut processed_results, &node, count, download, upload);
                    }
                }
            }
        }
        
        // 排序处理
        self.sort_and_limit_results(&mut processed_results, sort_by, sort_order, limit);
        
        Ok(processed_results)
    }
    
    // 聚合节点统计数据
    fn aggregate_node_stats(&self, results: &mut Vec<JsonValue>, node: &str, count: i64, download: i64, upload: i64) {
        // 查找是否已有该节点的统计
        let mut found = false;
        for result in results.iter_mut() {
            if let Some(obj) = result.as_object_mut() {
                if let Some(node_val) = obj.get("node") {
                    if node_val.as_str() == Some(node) {
                        // 更新现有节点统计
                        found = true;
                        
                        // 更新计数
                        if let Some(entry_count) = obj.get("count").and_then(|v| v.as_i64()) {
                            obj.insert("count".to_string(), JsonValue::Number((entry_count + count).into()));
                        }
                        
                        // 更新下载
                        if let Some(entry_download) = obj.get("download").and_then(|v| v.as_i64()) {
                            obj.insert("download".to_string(), JsonValue::Number((entry_download + download).into()));
                        }
                        
                        // 更新上传
                        if let Some(entry_upload) = obj.get("upload").and_then(|v| v.as_i64()) {
                            obj.insert("upload".to_string(), JsonValue::Number((entry_upload + upload).into()));
                        }
                        
                        // 更新总量
                        let new_download = obj.get("download").and_then(|v| v.as_i64()).unwrap_or(0);
                        let new_upload = obj.get("upload").and_then(|v| v.as_i64()).unwrap_or(0);
                        obj.insert("total".to_string(), JsonValue::Number((new_download + new_upload).into()));
                        
                        break;
                    }
                }
            }
        }
        
        // 如果没有找到，添加新的节点统计
        if !found {
            results.push(serde_json::json!({
                "node": node,
                "count": count,
                "download": download,
                "upload": upload,
                "total": download + upload
            }));
        }
    }
    
    // 排序和限制结果
    fn sort_and_limit_results(&self, results: &mut Vec<JsonValue>, sort_by: Option<&str>, sort_order: Option<&str>, limit: Option<u32>) {
        // 应用排序
        if let Some(sort_field) = sort_by {
            let ascending = sort_order == Some("asc");
            
            results.sort_by(|a, b| {
                let a_val = a.get(sort_field).and_then(|v| v.as_i64()).unwrap_or(0);
                let b_val = b.get(sort_field).and_then(|v| v.as_i64()).unwrap_or(0);
                
                if ascending {
                    a_val.cmp(&b_val)
                } else {
                    b_val.cmp(&a_val)
                }
            });
        } else {
            // 默认按count降序排序
            results.sort_by(|a, b| {
                let a_count = a.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                let b_count = b.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                b_count.cmp(&a_count)
            });
        }
        
        // 应用限制
        if let Some(limit) = limit {
            let limit = limit as usize;
            if limit < results.len() {
                results.truncate(limit);
            }
        }
    }
    
    // 处理GEOIP分组统计
    pub async fn execute_geoip_stats(&self, sql: &str, params: &[String], sort_by: Option<&str>, sort_order: Option<&str>, limit: Option<u32>) -> JsonResult {
        // 执行查询获取原始连接数据
        let connections = self.execute_connections_query(sql, params).await?;
        
        // 创建一个HashMap来累计每个geoip的统计信息
        let mut geoip_stats: HashMap<String, (i64, i64, i64)> = HashMap::new();
        
        // 处理每个连接
        for conn in connections {
            if let Some(obj) = conn.as_object() {
                let download = obj.get("download").and_then(|v| v.as_i64()).unwrap_or(0);
                let upload = obj.get("upload").and_then(|v| v.as_i64()).unwrap_or(0);
                
                // 尝试从destination_geoip字段获取geoip列表
                if let Some(geoip_value) = obj.get("destination_geoip") {
                    if let Some(geoip_str) = geoip_value.as_str() {
                        // 判断字符串是否为空或null
                        if geoip_str.is_empty() || geoip_str == "null" {
                            // 将空值单独列为一个组，而不是跳过
                            let entry = geoip_stats.entry("GEOIP为空".to_string()).or_insert((0, 0, 0));
                            entry.0 += 1; // 计数
                            entry.1 += download; // 下载
                            entry.2 += upload; // 上传
                            continue;
                        }
                        
                        // 解析JSON数组
                        if let Ok(geo_array) = serde_json::from_str::<Vec<String>>(geoip_str) {
                            for item in geo_array {
                                // 处理数组中的每一项
                                let entry = geoip_stats.entry(item).or_insert((0, 0, 0));
                                entry.0 += 1; // 计数
                                entry.1 += download; // 下载
                                entry.2 += upload; // 上传
                            }
                            continue;
                        }
                        
                        // 如果解析失败，将其归为未知类别
                        let entry = geoip_stats.entry("未知格式".to_string()).or_insert((0, 0, 0));
                        entry.0 += 1;
                        entry.1 += download;
                        entry.2 += upload;
                    }
                }
            }
        }
        
        // 如果没有找到任何geoip数据，添加一个"未知"项，避免返回空结果
        if geoip_stats.is_empty() {
            geoip_stats.insert("unknown".to_string(), (0, 0, 0));
        }
        
        // 将HashMap转换为结果列表
        let mut results: Vec<JsonValue> = geoip_stats.iter().map(|(geoip, (count, download, upload))| {
            serde_json::json!({
                "destination_geoip": geoip,
                "count": count,
                "download": download,
                "upload": upload,
                "total": download + upload
            })
        }).collect();
        
        // 应用排序和限制
        self.sort_and_limit_results(&mut results, sort_by, sort_order, limit);
        
        Ok(results)
    }
    
    // 专门处理host分组查询
    pub async fn execute_host_group_query(&self, sql: &str, params: &[String]) -> JsonResult {
        self.execute_query(sql, params, |row| {
            let mut result_obj = serde_json::Map::new();
            
            // 直接通过列名获取
            if let Ok(host) = row.try_get::<String, _>("host_display") {
                result_obj.insert("host".to_string(), JsonValue::String(host));
            }
            
            if let Ok(count) = row.try_get::<i64, _>("count") {
                result_obj.insert("count".to_string(), JsonValue::Number(count.into()));
            }
            
            if let Ok(download) = row.try_get::<i64, _>("download") {
                result_obj.insert("download".to_string(), JsonValue::Number(download.into()));
            }
            
            if let Ok(upload) = row.try_get::<i64, _>("upload") {
                result_obj.insert("upload".to_string(), JsonValue::Number(upload.into()));
            }
            
            // 计算总流量
            let download = row.try_get::<i64, _>("download").unwrap_or(0);
            let upload = row.try_get::<i64, _>("upload").unwrap_or(0);
            result_obj.insert("total".to_string(), JsonValue::Number((download + upload).into()));
            
            JsonValue::Object(result_obj)
        }).await
    }

    // 执行时间序列查询
    pub async fn execute_timeseries_query(&self, sql: &str, params: &[String]) -> JsonResult {
        self.execute_query(sql, params, |row| {
            let time_point: String = row.get(0);
            let value: i64 = row.get(1);
            
            serde_json::json!({
                "time": time_point,
                "value": value
            })
        }).await
    }

    // 执行连接查询
    pub async fn execute_connections_query(&self, sql: &str, params: &[String]) -> JsonResult {
        self.execute_query(sql, params, |row| {
            let mut result_obj = serde_json::Map::new();
            
            // 使用函数来提取和添加字段，减少重复代码
            let extract_and_add = |field: &str, obj: &mut serde_json::Map<String, JsonValue>| {
                if let Ok(value) = row.try_get::<String, _>(field) {
                    obj.insert(field.to_string(), JsonValue::String(value));
                } else if let Ok(value) = row.try_get::<i64, _>(field) {
                    obj.insert(field.to_string(), JsonValue::Number(value.into()));
                }
            };
            
            // 添加所有标准字段
            for field in &[
                "id", "download", "upload", "last_updated", "start", "network", 
                "conn_type", "source_ip", "destination_ip", "destination_geoip", 
                "source_port", "destination_port", "host", "process", "chains", 
                "rule", "rule_payload", "agent_id"
            ] {
                extract_and_add(field, &mut result_obj);
            }
            
            JsonValue::Object(result_obj)
        }).await
    }

    // 执行筛选选项查询
    pub async fn execute_filter_options_query(&self, sql: &str, params: &[String]) -> JsonResult {
        self.execute_query(sql, params, |row| {
            // 获取第一列的值作为选项
            let column_name = row.column(0).name();
            let value = Self::get_row_value(&row, 0);
            
            // 为空值使用特殊标记，以便前端可以显示
            let display_value = match &value {
                JsonValue::String(s) if s.is_empty() => JsonValue::String("(空)".to_string()),
                JsonValue::Null => JsonValue::String("(空)".to_string()),
                _ => value.clone(),
            };
            
            // 创建选项对象
            serde_json::json!({
                "value": value,
                "label": display_value,
                "field": column_name
            })
        }).await
    }

    // ==================== 代理节点相关方法 ====================
    
    // 获取所有代理节点列表
    pub async fn get_agents(&self) -> JsonResult {
        // SQL查询：获取所有不同的代理节点ID和最后更新时间
        let sql = r#"
            SELECT 
                agent_id,
                MAX(last_updated) as last_active,
                COUNT(*) as connections_count,
                SUM(conn_download) as total_download,
                SUM(conn_upload) as total_upload
            FROM connections
            WHERE agent_id IS NOT NULL
            GROUP BY agent_id
            ORDER BY last_active DESC
        "#;
        
        // 执行查询并转换结果
        self.execute_query(sql, &[], |row| {
            let agent_id: String = row.get("agent_id");
            let last_active: String = row.get("last_active");
            let connections_count: i64 = row.get("connections_count");
            let total_download: i64 = row.get("total_download");
            let total_upload: i64 = row.get("total_upload");
            
            // 创建代理节点记录
            serde_json::json!({
                "id": agent_id,
                "last_active": last_active,
                "connections_count": connections_count,
                "total_download": total_download,
                "total_upload": total_upload,
                "total_traffic": total_download + total_upload,
                "status": "unknown" // 状态将通过单独的API更新
            })
        }).await
    }
    
    // 获取单个代理节点状态
    pub async fn get_agent_status(&self, agent_id: &str) -> QueryResult<JsonValue> {
        // 首先查询该代理的基本信息
        let sql = r#"
            SELECT 
                agent_id,
                MAX(last_updated) as last_active,
                COUNT(*) as connections_count,
                SUM(conn_download) as total_download,
                SUM(conn_upload) as total_upload
            FROM connections
            WHERE agent_id = ?
            GROUP BY agent_id
        "#;
        
        // 执行查询
        let row_result = sqlx::query(sql)
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await?;
        
        if let Some(row) = row_result {
            let last_active: String = row.get("last_active");
            let connections_count: i64 = row.get("connections_count");
            let total_download: i64 = row.get("total_download");
            let total_upload: i64 = row.get("total_upload");
            
            // 计算是否活跃 - 如果最后活动时间在10分钟内
            let now = Utc::now();
            // 解析字符串为DateTime<Utc>
            let last_active_time = match chrono::DateTime::parse_from_rfc3339(&last_active) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => now,
            };
            let is_active = now.signed_duration_since(last_active_time).num_minutes() < 10;
            
            // 获取网络和规则分布
            let networks = self.get_agent_networks(agent_id).await?;
            let rules = self.get_agent_rules(agent_id).await?;
            
            // 返回完整状态
            Ok(serde_json::json!({
                "id": agent_id,
                "last_active": last_active,
                "connections_count": connections_count,
                "total_download": total_download,
                "total_upload": total_upload,
                "total_traffic": total_download + total_upload,
                "is_active": is_active,
                "status": if is_active { "active" } else { "inactive" },
                "networks": networks,
                "rules": rules
            }))
        } else {
            // 如果没有找到代理节点
            Err(SqlxError::RowNotFound)
        }
    }
    
    // 获取代理节点的网络类型分布
    async fn get_agent_networks(&self, agent_id: &str) -> JsonResult {
        let sql = r#"
            SELECT 
                network,
                COUNT(*) as count
            FROM connections
            WHERE agent_id = ?
            GROUP BY network
        "#;
        
        self.execute_query(sql, &[agent_id.to_string()], |row| {
            let network: String = row.get("network");
            let count: i64 = row.get("count");
            
            serde_json::json!({
                "network": network,
                "count": count
            })
        }).await
    }
    
    // 获取代理节点的规则分布
    async fn get_agent_rules(&self, agent_id: &str) -> JsonResult {
        let sql = r#"
            SELECT 
                rule,
                COUNT(*) as count
            FROM connections
            WHERE agent_id = ?
            GROUP BY rule
        "#;
        
        self.execute_query(sql, &[agent_id.to_string()], |row| {
            let rule: String = row.get("rule");
            let count: i64 = row.get("count");
            
            serde_json::json!({
                "rule": rule,
                "count": count
            })
        }).await
    }

    // ==================== 维护方法 ====================

    // 清理已同步且超过保留期限的旧数据
    pub async fn cleanup_old_records(&self, days_to_keep: i64) -> QueryResult<i64> {
        // 获取删除条件：
        // 1. 已同步完成（last_updated <= last_sync_time）
        // 2. 超过保留期限（start < 当前时间 - days_to_keep天）
        let sql = r#"
            DELETE FROM connections 
            WHERE last_updated <= (SELECT last_sync_time FROM sync_state WHERE id = 1)
            AND start < datetime('now', ?, 'utc')
        "#;
        
        let days_param = format!("-{} day", days_to_keep);
        
        // 执行删除操作
        let result = sqlx::query(sql)
            .bind(days_param)
            .execute(&self.pool)
            .await?;
            
        Ok(result.rows_affected() as i64)
    }
}

// 添加过滤条件 (完整版本，支持通配符和非通配符)
pub fn add_filter_condition_with_wildcards(sql: &mut String, params: &mut Vec<String>, field: &str, value: &Option<String>, operator: &str, use_wildcards: bool) {
    if let Some(value) = value {
        sql.push_str(&format!(" AND {} {} ?", field, operator));
        
        if use_wildcards && operator == "LIKE" {
            params.push(format!("%{}%", value));
        } else {
            params.push(value.clone());
        }
    }
}

// 添加过滤条件 (简化版本，无通配符)
pub fn add_filter_condition(sql: &mut String, params: &mut Vec<String>, field: &str, value: &Option<String>, operator: &str) {
    if let Some(value) = value {
        sql.push_str(&format!(" AND {} {} ?", field, operator));
        params.push(value.clone());
    }
} 