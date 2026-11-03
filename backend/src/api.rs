use crate::common::{GlobalData, SyncPackage};
use std::error::Error;
use std::time::Duration;
use std::convert::Infallible;
use tokio_tungstenite::connect_async;
use futures_util::StreamExt;
use reqwest::Client;
use std::sync::Arc;
use warp::Filter;
use warp::http::StatusCode;
use warp::reply::Json;
use serde_json::json;
use serde::{Deserialize, Serialize};
use warp::http::Method;
use warp::http::header::{AUTHORIZATION, CONTENT_TYPE};

// Mihomo API客户端
pub struct MihomoClient {
    host: String,
    port: u16,
    token: String,
}

impl MihomoClient {
    pub fn new(host: String, port: u16, token: String) -> Self {
        Self { host, port, token }
    }

    // 连接到Mihomo WebSocket API并处理数据，使用同步回调函数
    pub async fn connect<F>(&self, mut callback: F) -> Result<(), Box<dyn Error>>
    where
        F: FnMut(GlobalData) -> Result<(), String> + Send + 'static,
    {
        let ws_uri = format!(
            "ws://{}:{}/connections?token={}",
            self.host, self.port, self.token
        );

        println!("连接到Mihomo API: {}:{}...", self.host, self.port);

        loop {
            match connect_async(&ws_uri).await {
                Ok((ws_stream, _)) => {
                    println!("已连接到Mihomo API");
                    let (_, mut read) = ws_stream.split();
                    
                    // 直接等待消息，不设置超时
                    loop {
                        match read.next().await {
                            Some(Ok(message)) => {
                                if message.is_text() {
                                    let text = message.into_text()?;
                                    match serde_json::from_str::<GlobalData>(&text) {
                                        Ok(data) => {
                                            if let Err(e) = callback(data) {
                                                println!("处理数据出错: {}", e);
                                            }
                                        },
                                        Err(e) => {
                                            println!("解析消息失败: {}\n原始数据: {}", e, text);
                                        }
                                    };
                                }
                            },
                            Some(Err(e)) => {
                                println!("WebSocket错误，重新连接: {:?}", e);
                                break;
                            },
                            None => {
                                // 流结束，这表示连接已经正常关闭
                                println!("WebSocket连接已关闭，重新连接中...");
                                break;
                            }
                        }
                    }
                },
                Err(e) => {
                    println!("连接错误: {:?}, 重试中...", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}

// 主节点API客户端
pub struct MasterClient {
    client: Client,
    master_url: String,
    master_token: Option<String>,
}

impl MasterClient {
    pub fn new(master_url: String, master_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("创建HTTP客户端失败");
        
        Self { client, master_url, master_token }
    }

    // 检查主节点是否在线
    pub async fn is_online(&self) -> bool {
        let health_url = format!("{}/api/v1/health", self.master_url);
        match self.client.get(&health_url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    // 同步数据到主节点
    pub async fn sync_data(&self, package: &SyncPackage) -> Result<(), Box<dyn Error + Send + Sync>> {
        let sync_url = format!("{}/api/v1/sync", self.master_url);
        let mut request = self.client.post(&sync_url);
        
        if let Some(token) = &self.master_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }
        
        let response = request
            .json(package)
            .send()
            .await?;
            
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("同步失败: {} - {}", status, text).into());
        }
        
        Ok(())
    }
}

// API 响应结构
#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    status: String,
    data: Option<T>,
    message: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    // 创建成功响应
    fn success(data: T) -> Json {
        warp::reply::json(&json!({
            "status": "success",
            "data": data,
            "message": null
        }))
    }
    
    // 创建错误响应
    fn error(message: &str) -> Json {
        warp::reply::json(&json!({
            "status": "error",
            "data": null,
            "message": message
        }))
    }
}

// 统一的API响应类型
type ApiResult = Result<Json, warp::Rejection>;

// API错误类型

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,                // 未授权
    BadRequest(String),          // 请求参数错误
    DatabaseError(String),       // 数据库错误
    NotFound,                    // 资源不存在
    InternalError(String),       // 内部服务器错误
}

impl warp::reject::Reject for ApiError {}

// 统一的统计查询参数
#[derive(Deserialize, Debug, Clone)]
pub struct StatsQuery {
    r#type: String,               // 统计类型: summary, group, timeseries
    group_by: Option<String>,     // 分组字段(用于group类型): network, rule, process, destination, geoip
    interval: Option<String>,     // 时间间隔(用于timeseries类型): hour, day, week, month
    metric: Option<String>,       // 统计指标: connections, download, upload, total
    from: Option<String>,         // 开始时间，ISO 8601格式
    to: Option<String>,           // 结束时间，ISO 8601格式
    agent_id: Option<String>,     // 代理ID过滤
    network: Option<String>,      // 网络类型过滤
    rule: Option<String>,         // 规则过滤
    process: Option<String>,      // 进程名过滤
    destination: Option<String>,  // 目标地址过滤
    geoip: Option<String>,        // 国家/地区过滤
    limit: Option<u32>,           // 限制返回结果数量
    sort_by: Option<String>,      // 排序字段
    sort_order: Option<String>,   // 排序顺序 (asc, desc)
}

// 连接查询参数
#[derive(Deserialize, Debug, Clone)]
pub struct ConnectionsQuery {
    agent_id: Option<String>,     // 代理ID过滤
    network: Option<String>,      // 网络类型过滤
    rule: Option<String>,         // 规则过滤
    process: Option<String>,      // 进程名过滤
    destination: Option<String>,  // 目标地址过滤
    host: Option<String>,         // 主机名过滤
    geoip: Option<String>,        // 国家/地区过滤
    from: Option<String>,         // 开始时间，ISO 8601格式
    to: Option<String>,           // 结束时间，ISO 8601格式
    limit: Option<u32>,           // 限制返回结果数量
    offset: Option<u32>,          // 分页偏移量
    sort_by: Option<String>,      // 排序字段
    sort_order: Option<String>,   // 排序顺序 (asc, desc)
}

// 筛选器选项查询参数
#[derive(Deserialize, Debug, Clone)]
pub struct FilterOptionsQuery {
    filter_type: String,          // 筛选类型: agent_id, network, rule, process, destination, host, geoip
    query: Option<String>,        // 可选的查询字符串，用于搜索特定的选项
    limit: Option<u32>,           // 限制返回结果数量
}

// 主节点API服务器
pub struct MasterServer {
    database: Arc<crate::db::Database>,
    api_token: Option<String>,
}

impl MasterServer {
    pub fn new(database: Arc<crate::db::Database>, api_token: Option<String>) -> Self {
        Self { database, api_token }
    }

    // 构建API认证过滤器
    fn with_auth(&self) -> impl Filter<Extract = (), Error = warp::Rejection> + Clone {
        let token = self.api_token.clone();
        
        warp::header::optional::<String>("authorization")
            .and_then(move |auth: Option<String>| {
                let token = token.clone();
                async move {
                    // 如果未设置令牌，则无需验证
                    if token.is_none() {
                        return Ok(());
                    }
                    
                    match auth {
                        Some(auth_header) if auth_header.starts_with("Bearer ") => {
                            let provided_token = auth_header[7..].to_string();
                            if provided_token == token.unwrap() {
                                Ok(())
                            } else {
                                Err(warp::reject::custom(ApiError::Unauthorized))
                            }
                        }
                        _ => Err(warp::reject::custom(ApiError::Unauthorized)),
                    }
                }
            })
            .untuple_one()
    }

    // 构建数据库访问过滤器
    fn with_db(&self) -> impl Filter<Extract = (Arc<crate::db::Database>,), Error = Infallible> + Clone {
        let db = self.database.clone();
        warp::any().map(move || db.clone())
    }

    // 启动API服务器
    pub async fn start(&self, host: &str, port: u16) -> Result<(), Box<dyn Error>> {
        // 构建所有API路由
        let routes = self.build_routes();
            
        println!("启动主节点API服务器在 http://{}:{}...", host, port);
        let socket_addr: std::net::SocketAddr = format!("{}:{}", host, port)
            .parse()
            .map_err(|e| format!("无效的地址: {}", e))?;
            
        warp::serve(routes)
            .run(socket_addr)
            .await;
            
        Ok(())
    }
    
    // 构建所有API路由
    fn build_routes(&self) -> impl Filter<Extract = impl warp::Reply, Error = Infallible> + Clone {
        // 健康检查路由
        let health_route = warp::path!("api" / "v1" / "health")
            .and(warp::get())
            .map(|| "OK");
        
        // 同步路由
        let sync_route = warp::path!("api" / "v1" / "sync")
            .and(warp::post())
            .and(self.with_auth())
            .and(warp::body::json())
            .and(self.with_db())
            .and_then(|sync_package: SyncPackage, db: Arc<crate::db::Database>| async move {
                handle_sync(sync_package, db).await
            });
        
        // 统计路由 - 统一入口
        let stats_route = warp::path!("api" / "v1" / "stats")
            .and(warp::get())
            .and(self.with_auth())
            .and(warp::query::<StatsQuery>())
            .and(self.with_db())
            .and_then(|query: StatsQuery, db: Arc<crate::db::Database>| async move {
                handle_stats(query, db).await
            });

        // 连接查询路由
        let connections_route = warp::path!("api" / "v1" / "connections")
            .and(warp::get())
            .and(self.with_auth())
            .and(warp::query::<ConnectionsQuery>())
            .and(self.with_db())
            .and_then(|query: ConnectionsQuery, db: Arc<crate::db::Database>| async move {
                handle_connections(query, db).await
            });

        // 代理节点查询路由
        let agents_route = warp::path!("api" / "v1" / "agents")
            .and(warp::get())
            .and(self.with_auth())
            .and(self.with_db())
            .and_then(|db: Arc<crate::db::Database>| async move {
                handle_agents(db).await
            });

        // 代理节点状态查询路由
        let agent_status_route = warp::path!("api" / "v1" / "agents" / String / "status")
            .and(warp::get())
            .and(self.with_auth())
            .and(self.with_db())
            .and_then(|agent_id: String, db: Arc<crate::db::Database>| async move {
                handle_agent_status(agent_id, db).await
            });

        // 筛选器选项查询路由
        let filter_options_route = warp::path!("api" / "v1" / "filter-options")
            .and(warp::get())
            .and(self.with_auth())
            .and(warp::query::<FilterOptionsQuery>())
            .and(self.with_db())
            .and_then(|query: FilterOptionsQuery, db: Arc<crate::db::Database>| async move {
                handle_filter_options(query, db).await
            });

        // 配置 CORS
        let cors = warp::cors()
            .allow_any_origin()
            .allow_methods(vec![Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers(vec![CONTENT_TYPE, AUTHORIZATION])
            .allow_credentials(false);
            
        // 合并所有路由并添加 CORS 和错误处理
        health_route
            .or(sync_route)
            .or(stats_route)
            .or(connections_route)
            .or(agents_route)
            .or(agent_status_route)
            .or(filter_options_route)
            .with(cors)
            .recover(handle_rejection)
    }
}

// 处理统计请求的统一入口
async fn handle_stats(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 根据统计类型分发到不同的处理函数
    match query.r#type.as_str() {
        "summary" => handle_stats_summary(query, db).await,
        "group" => handle_stats_group(query, db).await,
        "timeseries" => handle_stats_timeseries(query, db).await,
        _ => Err(warp::reject::custom(ApiError::BadRequest(
            format!("不支持的统计类型: {}", query.r#type)
        )))
    }
}

// 处理汇总统计请求
async fn handle_stats_summary(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 构建基础SQL和参数
    let (count_sql, count_params) = build_base_filter(
        "SELECT COUNT(*) as count FROM connections",
        &query
    );
    
    let (traffic_sql, traffic_params) = build_base_filter(
        "SELECT SUM(conn_download) as download, SUM(conn_upload) as upload FROM connections",
        &query
    );
    
    // 执行查询
    let count_result = db.execute_count_query(&count_sql, &count_params).await;
    let traffic_result = db.execute_traffic_query(&traffic_sql, &traffic_params).await;
    
    match (count_result, traffic_result) {
        (Ok(count), Ok((download, upload))) => {
            Ok(ApiResponse::success(json!({
                "count": count,
                "download": download,
                "upload": upload,
                "total": download + upload
            })))
        },
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("统计查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("统计查询失败: {}", e)
            )))
        }
    }
}

// 处理分组统计请求
async fn handle_stats_group(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 检查是否指定了分组字段
    let group_field = match &query.group_by {
        Some(field) => field.as_str(),
        None => return Err(warp::reject::custom(ApiError::BadRequest(
            "分组统计必须指定 group_by 参数".to_string()
        )))
    };
    
    // 根据分组类型分发到专门的处理函数
    match group_field {
        "geoip" => handle_geoip_group(query.clone(), db).await,
        "host" => handle_host_group(query.clone(), db).await,
        "chains" => handle_chains_group(query.clone(), db).await,
        "rule" => handle_rule_group(query.clone(), db).await,
        _ => handle_standard_group(query.clone(), db, group_field).await
    }
}

// 处理GEOIP分组
async fn handle_geoip_group(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 查询原始连接数据
    let base_sql = "SELECT id, destination_geoip, conn_download as download, conn_upload as upload FROM connections";
    let (sql, params) = build_base_filter(base_sql, &query);
    
    // 执行GEOIP分组统计
    match db.execute_geoip_stats(&sql, &params, query.sort_by.as_deref(), query.sort_order.as_deref(), query.limit).await {
        Ok(results) => Ok(ApiResponse::success(results)),
        Err(e) => {
            eprintln!("GEOIP分组查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("GEOIP分组查询失败: {}", e)
            )))
        }
    }
}

// 处理主机分组
async fn handle_host_group(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 构建主机查询SQL
    let base_sql = r#"
        SELECT 
            COALESCE(NULLIF(host, ''), destination_ip) as host_display,
                COUNT(*) as count, 
                SUM(conn_download) as download, 
                SUM(conn_upload) as upload 
        FROM connections
    "#;
    
    let (mut sql, params) = build_base_filter(base_sql, &query);
    
    // 添加分组和排序
    sql.push_str(" GROUP BY host_display");
    
    add_sort_clause(&mut sql, &query);
    add_limit_clause(&mut sql, &query);
    
    // 执行查询
    match db.execute_host_group_query(&sql, &params).await {
        Ok(results) => Ok(ApiResponse::success(results)),
        Err(e) => {
            eprintln!("主机分组查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("主机分组查询失败: {}", e)
            )))
        }
    }
}

// 处理代理链路分组
async fn handle_chains_group(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 构建链路查询SQL
    let base_sql = r#"
        SELECT 
            chains,
            COUNT(*) as count, 
            SUM(conn_download) as download, 
            SUM(conn_upload) as upload 
        FROM connections
    "#;
    
    let (mut sql, params) = build_base_filter(base_sql, &query);
    
    // 添加分组和排序
    sql.push_str(" GROUP BY chains");
    
    add_sort_clause(&mut sql, &query);
    add_limit_clause(&mut sql, &query);
    
    // 执行查询
    match db.execute_group_query(&sql, &params).await {
        Ok(results) => {
            // 处理链路结果，提取最后节点
            match db.process_chains_results(results, query.sort_by.as_deref(), query.sort_order.as_deref(), query.limit).await {
                Ok(processed) => Ok(ApiResponse::success(processed)),
                Err(e) => {
                    eprintln!("链路分组结果处理错误: {:?}", e);
                    Err(warp::reject::custom(ApiError::DatabaseError(
                        format!("链路分组结果处理失败: {}", e)
                    )))
                }
            }
        },
        Err(e) => {
            eprintln!("链路分组查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("链路分组查询失败: {}", e)
            )))
        }
    }
}

// 处理规则分组
async fn handle_rule_group(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 构建规则查询SQL
    let base_sql = r#"
        SELECT 
            rule, rule_payload, 
            COUNT(*) as count, 
            SUM(conn_download) as download, 
            SUM(conn_upload) as upload 
        FROM connections
    "#;
    
    let (mut sql, params) = build_base_filter(base_sql, &query);
    
    // 添加分组和排序
    sql.push_str(" GROUP BY rule, rule_payload");
    
    add_sort_clause(&mut sql, &query);
    add_limit_clause(&mut sql, &query);
    
    // 执行查询
    match db.execute_group_query(&sql, &params).await {
        Ok(results) => Ok(ApiResponse::success(results)),
        Err(e) => {
            eprintln!("规则分组查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("规则分组查询失败: {}", e)
            )))
        }
    }
}

// 处理标准分组
async fn handle_standard_group(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
    group_field: &str,
) -> ApiResult {
    // 确定分组字段名和显示名称
    let (column_name, sql_expression) = match group_field {
        "network" => ("network", "network"),
        "process" => ("process", "CASE WHEN process = '' OR process IS NULL THEN '进程为空' ELSE process END as process"),
        "destination" => ("destination_ip", "destination_ip"),
        _ => return Err(warp::reject::custom(ApiError::BadRequest(
            format!("不支持的分组字段: {}", group_field)
        ))),
    };
    
    // 构建查询SQL
    let base_sql = format!(
        "SELECT {}, COUNT(*) as count, SUM(conn_download) as download, SUM(conn_upload) as upload FROM connections",
        sql_expression
    );
    
    let (mut sql, params) = build_base_filter(&base_sql, &query);
    
    // 添加分组和排序
    sql.push_str(&format!(" GROUP BY {}", column_name));
    
    add_sort_clause(&mut sql, &query);
    add_limit_clause(&mut sql, &query);
    
    // 执行查询
    match db.execute_group_query(&sql, &params).await {
        Ok(results) => Ok(ApiResponse::success(results)),
        Err(e) => {
            eprintln!("标准分组查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("标准分组查询失败: {}", e)
            )))
        }
    }
}

// 添加排序子句
fn add_sort_clause(sql: &mut String, query: &StatsQuery) {
                if let Some(sort_by) = &query.sort_by {
                    let sort_field = match sort_by.as_str() {
                        "count" => "count",
                        "download" => "download",
                        "upload" => "upload",
            "total" => "(download + upload)",
                        _ => "count"
                    };
                    
                    let sort_order = match query.sort_order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC"
        };
        
        sql.push_str(&format!(" ORDER BY {} {}", sort_field, sort_order));
                        } else {
        // 默认排序
        sql.push_str(" ORDER BY count DESC");
                        }
                }
                
// 添加限制子句
fn add_limit_clause(sql: &mut String, query: &StatsQuery) {
                if let Some(limit) = query.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }
}

// 处理时间序列统计请求
async fn handle_stats_timeseries(
    query: StatsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 确保有开始和结束时间
    let from = match &query.from {
        Some(time) => time.clone(),
        None => return Err(warp::reject::custom(ApiError::BadRequest(
            "时间序列查询必须指定 from 参数".to_string()
        )))
    };
    
    let to = match &query.to {
        Some(time) => time.clone(),
        None => return Err(warp::reject::custom(ApiError::BadRequest(
            "时间序列查询必须指定 to 参数".to_string()
        )))
    };
    
    // 获取时间间隔和指标
    let interval = query.interval.as_deref().unwrap_or("day");
    let metric = query.metric.as_deref().unwrap_or("connections");
    
    // 根据选择的时间间隔确定SQL时间格式
    let time_format = match interval {
        "hour" => "%Y-%m-%d %H:00:00",
        "day" => "%Y-%m-%d",
        "week" => "%Y-%W",
        "month" => "%Y-%m",
        _ => return Err(warp::reject::custom(ApiError::BadRequest(
            format!("不支持的时间间隔: {}", interval)
        )))
    };
    
    // 根据选择的指标确定要计算的字段
    let (select_expr, group_expr) = match metric {
        "connections" => ("COUNT(*) as value", ""),
        "download" => ("SUM(conn_download) as value", ""),
        "upload" => ("SUM(conn_upload) as value", ""),
        "total" => ("SUM(conn_download + conn_upload) as value", ""),
        _ => return Err(warp::reject::custom(ApiError::BadRequest(
            format!("不支持的指标: {}", metric)
        )))
    };
    
    // 构建查询SQL
    let base_sql = format!(
        "SELECT strftime('{}', datetime(last_updated, 'utc')) as time_point, {} FROM connections",
        time_format, select_expr
    );
    
    let (mut sql, mut params) = build_base_filter(&base_sql, &query);
    
    // 确保有时间范围过滤
    if !params.iter().any(|p| p == &from) {
        sql.push_str(" AND last_updated >= ?");
        params.push(from.clone());
    }
    
    if !params.iter().any(|p| p == &to) {
        sql.push_str(" AND last_updated <= ?");
        params.push(to.clone());
    }
    
    // 添加分组
    sql.push_str(&format!(" GROUP BY time_point{}", group_expr));
    sql.push_str(" ORDER BY time_point ASC");
    
    // 执行查询
    match db.execute_timeseries_query(&sql, &params).await {
        Ok(results) => {
            Ok(ApiResponse::success(results))
        },
        Err(e) => {
            eprintln!("时间序列查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("时间序列查询失败: {}", e)
            )))
        }
    }
}

// 构建基础SQL查询过滤条件
fn build_base_filter(base_sql: &str, query: &StatsQuery) -> (String, Vec<String>) {
    let mut sql = format!("{} WHERE 1=1", base_sql);
    let mut params: Vec<String> = Vec::new();
    
    // 添加时间范围过滤
    if let Some(from) = &query.from {
        sql.push_str(" AND last_updated >= ?");
        params.push(from.clone());
    }
    
    if let Some(to) = &query.to {
        sql.push_str(" AND last_updated <= ?");
        params.push(to.clone());
    }
    
    // 添加其他筛选条件
    crate::db::add_filter_condition(&mut sql, &mut params, "agent_id", &query.agent_id, "=");
    crate::db::add_filter_condition(&mut sql, &mut params, "network", &query.network, "=");
    crate::db::add_filter_condition(&mut sql, &mut params, "rule", &query.rule, "=");
    crate::db::add_filter_condition_with_wildcards(&mut sql, &mut params, "process", &query.process, "LIKE", true);
    crate::db::add_filter_condition_with_wildcards(&mut sql, &mut params, "destination_ip", &query.destination, "LIKE", true);
    crate::db::add_filter_condition_with_wildcards(&mut sql, &mut params, "destination_geoip", &query.geoip, "LIKE", true);
    
    (sql, params)
}

// 处理同步请求
async fn handle_sync(
    sync_package: SyncPackage,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    match db.batch_upsert_records(&sync_package.connections).await {
        Ok(_) => {
            println!(
                "成功从节点 {} 同步了 {} 条记录",
                sync_package.agent_id,
                sync_package.connections.len()
            );
            Ok(ApiResponse::success(json!({
                "message": "数据同步成功",
                "count": sync_package.connections.len(),
            })))
        }
        Err(e) => {
            eprintln!("同步出错: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("数据同步失败: {}", e)
            )))
        }
    }
}

// 处理连接查询请求
async fn handle_connections(
    query: ConnectionsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 构建基础SQL
    let base_sql = r#"
        SELECT 
            id, conn_download as download, conn_upload as upload, 
            last_updated, start, network, type as conn_type, 
            source_ip, destination_ip, destination_geoip, source_port, 
            destination_port, COALESCE(NULLIF(host, ''), destination_ip) as host, 
            CASE WHEN process = '' OR process IS NULL THEN '进程为空' ELSE process END as process, 
            chains, rule, rule_payload, agent_id
        FROM connections
    "#;
    
    // 构建查询条件
    let (sql, params) = build_connections_filter(base_sql, &query);
    
    // 执行查询
    match db.execute_connections_query(&sql, &params).await {
        Ok(connections) => {
            Ok(ApiResponse::success(connections))
        },
        Err(e) => {
            eprintln!("连接查询错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("连接查询失败: {}", e)
            )))
        }
    }
}

// 构建连接查询条件
fn build_connections_filter(base_sql: &str, query: &ConnectionsQuery) -> (String, Vec<String>) {
    let mut sql = base_sql.to_string();
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    
    // 添加筛选条件
    if let Some(agent_id) = &query.agent_id {
        conditions.push("agent_id = ?".to_string());
        params.push(agent_id.clone());
    }
    
    if let Some(network) = &query.network {
        conditions.push("network = ?".to_string());
        params.push(network.clone());
    }
    
    if let Some(rule) = &query.rule {
        conditions.push("rule = ?".to_string());
        params.push(rule.clone());
    }
    
    if let Some(process) = &query.process {
        // 处理空进程的特殊情况
        if process == "进程为空" {
            conditions.push("(process = '' OR process IS NULL)".to_string());
        } else {
            conditions.push("process LIKE ?".to_string());
        params.push(format!("%{}%", process));
        }
    }
    
    if let Some(destination) = &query.destination {
        conditions.push("(destination_ip LIKE ? OR host LIKE ?)".to_string());
        params.push(format!("%{}%", destination));
        params.push(format!("%{}%", destination));
    }
    
    if let Some(host) = &query.host {
        conditions.push("host LIKE ?".to_string());
        params.push(format!("%{}%", host));
    }
    
    if let Some(geoip) = &query.geoip {
        conditions.push("destination_geoip LIKE ?".to_string());
        params.push(format!("%\"{}%", geoip)); // 使用JSON包含匹配
    }
    
    if let Some(from) = &query.from {
        conditions.push("start >= ?".to_string());
        params.push(from.clone());
    }
    
    if let Some(to) = &query.to {
        conditions.push("start <= ?".to_string());
        params.push(to.clone());
    }
    
    // 添加WHERE子句
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    
    // 添加排序
    if let Some(sort_by) = &query.sort_by {
        let column = match sort_by.as_str() {
            "download" => "conn_download",
            "upload" => "conn_upload",
            "start" => "start",
            "last_updated" => "last_updated",
            _ => "last_updated", // 默认按最后更新时间排序
        };
        
        let sort_order = match query.sort_order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        };
        
        sql.push_str(&format!(" ORDER BY {} {}", column, sort_order));
    } else {
        // 默认按最后更新时间排序
        sql.push_str(" ORDER BY last_updated DESC");
    }
    
    // 添加分页
    if let Some(limit) = query.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
        
        if let Some(offset) = query.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }
    } else {
        // 默认限制100条记录
        sql.push_str(" LIMIT 100");
    }
    
    (sql, params)
}

// 处理代理节点相关请求
async fn handle_agents(
    db: Arc<crate::db::Database>,
) -> ApiResult {
    match db.get_agents().await {
        Ok(agents) => {
            Ok(ApiResponse::success(agents))
        },
        Err(e) => {
            eprintln!("获取代理节点列表错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取代理节点列表失败: {}", e)
            )))
        }
    }
}

async fn handle_agent_status(
    agent_id: String,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    match db.get_agent_status(&agent_id).await {
        Ok(status) => {
            Ok(ApiResponse::success(status))
        },
        Err(e) => {
            eprintln!("获取代理节点状态错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取代理节点状态失败: {}", e)
            )))
        }
    }
}

// 处理筛选器选项请求
async fn handle_filter_options(
    query: FilterOptionsQuery,
    db: Arc<crate::db::Database>,
) -> ApiResult {
    // 根据筛选类型执行不同的查询
    match query.filter_type.as_str() {
        "agent_id" => get_agent_id_options(db, query.query, query.limit).await,
        "network" => get_network_options(db, query.query, query.limit).await,
        "rule" => get_rule_options(db, query.query, query.limit).await,
        "process" => get_process_options(db, query.query, query.limit).await,
        "destination" => get_destination_options(db, query.query, query.limit).await,
        "host" => get_host_options(db, query.query, query.limit).await,
        "geoip" => get_geoip_options(db, query.query, query.limit).await,
        _ => Err(warp::reject::custom(ApiError::BadRequest(
            format!("不支持的筛选类型: {}", query.filter_type)
        )))
    }
}

// 获取代理ID选项
async fn get_agent_id_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    let mut sql = "SELECT DISTINCT agent_id FROM connections WHERE agent_id IS NOT NULL".to_string();
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        sql.push_str(" AND agent_id LIKE ?");
        params.push(format!("%{}%", q));
    }
    
    sql.push_str(" ORDER BY agent_id");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取代理ID选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取代理ID选项失败: {}", e)
            )))
        }
    }
}

// 获取网络类型选项
async fn get_network_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    let mut sql = "SELECT DISTINCT network FROM connections WHERE network IS NOT NULL".to_string();
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        sql.push_str(" AND network LIKE ?");
        params.push(format!("%{}%", q));
    }
    
    sql.push_str(" ORDER BY network");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取网络类型选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取网络类型选项失败: {}", e)
            )))
        }
    }
}

// 获取规则选项
async fn get_rule_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    let mut sql = "SELECT DISTINCT rule FROM connections WHERE rule IS NOT NULL".to_string();
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        sql.push_str(" AND rule LIKE ?");
        params.push(format!("%{}%", q));
    }
    
    sql.push_str(" ORDER BY rule");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取规则选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取规则选项失败: {}", e)
            )))
        }
    }
}

// 获取进程选项
async fn get_process_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    let mut sql = "SELECT DISTINCT CASE WHEN process = '' OR process IS NULL THEN '进程为空' ELSE process END as process FROM connections".to_string();
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        if q == "进程为空" {
            sql.push_str(" WHERE (process = '' OR process IS NULL)");
        } else {
            sql.push_str(" WHERE process LIKE ?");
            params.push(format!("%{}%", q));
        }
    }
    
    sql.push_str(" ORDER BY process");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取进程选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取进程选项失败: {}", e)
            )))
        }
    }
}

// 获取目标地址选项
async fn get_destination_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    let mut sql = "SELECT DISTINCT destination_ip FROM connections WHERE destination_ip IS NOT NULL".to_string();
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        sql.push_str(" AND destination_ip LIKE ?");
        params.push(format!("%{}%", q));
    }
    
    sql.push_str(" ORDER BY destination_ip");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取目标地址选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取目标地址选项失败: {}", e)
            )))
        }
    }
}

// 获取主机选项
async fn get_host_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    let mut sql = "SELECT DISTINCT host FROM connections WHERE host IS NOT NULL AND host != ''".to_string();
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        sql.push_str(" AND host LIKE ?");
        params.push(format!("%{}%", q));
    }
    
    sql.push_str(" ORDER BY host");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取主机选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取主机选项失败: {}", e)
            )))
        }
    }
}

// 获取GeoIP选项
async fn get_geoip_options(
    db: Arc<crate::db::Database>,
    query: Option<String>,
    limit: Option<u32>,
) -> ApiResult {
    // 由于GeoIP存储为JSON，需要特殊处理
    let mut sql = r#"
    WITH extracted_geoip AS (
        SELECT DISTINCT 
            json_extract(destination_geoip, '$.country') as country,
            json_extract(destination_geoip, '$.countryCode') as country_code
        FROM connections 
        WHERE destination_geoip IS NOT NULL AND destination_geoip != '{}'
    )
    SELECT country || ' (' || country_code || ')' as geoip 
    FROM extracted_geoip
    WHERE country IS NOT NULL AND country_code IS NOT NULL
    "#.to_string();
    
    let mut params: Vec<String> = Vec::new();
    
    // 添加查询条件
    if let Some(q) = query {
        sql.push_str(" AND (country LIKE ? OR country_code LIKE ?)");
        params.push(format!("%{}%", q));
        params.push(format!("%{}%", q));
    }
    
    sql.push_str(" ORDER BY country");
    
    // 添加限制
    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }
    
    match db.execute_filter_options_query(&sql, &params).await {
        Ok(options) => Ok(ApiResponse::success(options)),
        Err(e) => {
            eprintln!("获取GeoIP选项错误: {:?}", e);
            Err(warp::reject::custom(ApiError::DatabaseError(
                format!("获取GeoIP选项失败: {}", e)
            )))
        }
    }
}

// 处理请求错误
async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, Infallible> {
    let (code, message) = if err.is_not_found() {
        (StatusCode::NOT_FOUND, "未找到请求的资源".to_string())
    } else if let Some(e) = err.find::<ApiError>() {
        match e {
            ApiError::Unauthorized => 
                (StatusCode::UNAUTHORIZED, "认证失败".to_string()),
            ApiError::BadRequest(msg) => 
                (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::DatabaseError(msg) => 
                (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ApiError::NotFound => 
                (StatusCode::NOT_FOUND, "未找到请求的资源".to_string()),
            ApiError::InternalError(msg) => 
                (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        }
    } else {
        eprintln!("未处理的拒绝: {:?}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, "内部服务器错误".to_string())
    };
    
    Ok(warp::reply::with_status(
        ApiResponse::<()>::error(&message),
        code
    ))
} 