use crate::{
    common::{local_day, month_start, Result},
    db::{Database, DetailFilter},
};
use chrono::{FixedOffset, Months, NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};
use warp::{http::StatusCode, Filter, Rejection, Reply};

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    Unauthorized,
    Database,
}
impl warp::reject::Reject for ApiError {}

fn bad_request(message: impl Into<String>) -> Rejection {
    warp::reject::custom(ApiError::BadRequest(message.into()))
}

fn response(result: Result<Value>) -> std::result::Result<warp::reply::Json, Rejection> {
    result
        .map(|data| warp::reply::json(&json!({"status": "success", "data": data, "message": null})))
        .map_err(|error| {
            eprintln!("查询数据库失败：{error}");
            warp::reject::custom(ApiError::Database)
        })
}

#[derive(Default, Deserialize)]
struct StatsQuery {
    dimension: Option<String>,
    period: Option<String>,
    name: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Default, Deserialize)]
struct DetailQuery {
    month: Option<String>,
    airport: Option<String>,
    group: Option<String>,
    domain: Option<String>,
    rule: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Default, Deserialize)]
struct AnomalyQuery {
    connection_id: Option<String>,
    reason: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Default, Deserialize)]
struct RangeQuery {
    from: Option<String>,
    to: Option<String>,
}

fn pagination(
    limit: Option<i64>,
    offset: Option<i64>,
) -> std::result::Result<(i64, i64), Rejection> {
    let (limit, offset) = (limit.unwrap_or(50), offset.unwrap_or(0));
    if !(1..=500).contains(&limit) || offset < 0 {
        return Err(bad_request("limit 必须为 1..500，offset 必须非负"));
    }
    Ok((limit, offset))
}

fn date(value: &str) -> std::result::Result<NaiveDate, Rejection> {
    let day = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| bad_request("日期必须为 YYYY-MM-DD"))?;
    if day.to_string() != value || !("0001-01-01"..="9999-12-31").contains(&value) {
        return Err(bad_request("日期必须为有效的 YYYY-MM-DD"));
    }
    Ok(day)
}

fn range(
    from: Option<&str>,
    to: Option<&str>,
) -> std::result::Result<(NaiveDate, NaiveDate), Rejection> {
    let start = month_start(local_day(Utc::now()));
    let from = from.map(date).transpose()?.unwrap_or(start);
    let to = to.map(date).transpose()?.unwrap_or(start + Months::new(1));
    if from >= to {
        return Err(bad_request("from 必须早于 to，范围左闭右开"));
    }
    Ok((from, to))
}

impl DetailQuery {
    fn validate(self) -> std::result::Result<DetailFilter, Rejection> {
        let from = match self.month {
            Some(month) => {
                if month.len() != 7 {
                    return Err(bad_request("month 必须为 YYYY-MM"));
                }
                date(&format!("{month}-01"))?
            }
            None => month_start(local_day(Utc::now())),
        };
        let (limit, offset) = pagination(self.limit, self.offset)?;
        Ok(DetailFilter {
            from: from.to_string(),
            to: (from + Months::new(1)).to_string(),
            airport: self.airport,
            group: self.group,
            domain: self.domain.map(|s| s.trim_end_matches('.').to_lowercase()),
            rule: self.rule,
            limit,
            offset,
        })
    }
}

pub fn routes(
    db: Arc<Database>,
    token: Option<String>,
) -> impl Filter<Extract = (impl Reply,), Error = Infallible> + Clone {
    let database = warp::any().map(move || db.clone());
    let auth = warp::header::optional::<String>("authorization")
        .and_then(move |header: Option<String>| {
            let token = token.clone();
            async move {
                if token.as_ref().is_some_and(|expected| {
                    header.as_deref() != Some(&format!("Bearer {expected}"))
                }) {
                    Err(warp::reject::custom(ApiError::Unauthorized))
                } else {
                    Ok(())
                }
            }
        })
        .untuple_one();
    let health = warp::path!("api" / "v1" / "health")
        .and(warp::get())
        .and(database.clone())
        .and_then(|db: Arc<Database>| async move { response(db.health().await) });
    let stats = warp::path!("api" / "v1" / "stats")
        .and(warp::get())
        .and(auth.clone())
        .and(warp::query::<StatsQuery>())
        .and(database.clone())
        .and_then(stats);
    let connections = warp::path!("api" / "v1" / "connections")
        .and(warp::get())
        .and(auth.clone())
        .and(warp::query::<DetailQuery>())
        .and(database.clone())
        .and_then(|query: DetailQuery, db: Arc<Database>| async move {
            response(db.connections(&query.validate()?).await)
        });
    let domains = warp::path!("api" / "v1" / "domains")
        .and(warp::get())
        .and(auth.clone())
        .and(warp::query::<DetailQuery>())
        .and(database.clone())
        .and_then(|query: DetailQuery, db: Arc<Database>| async move {
            response(db.domains(&query.validate()?).await)
        });
    let anomalies = warp::path!("api" / "v1" / "anomalies")
        .and(warp::get())
        .and(auth.clone())
        .and(warp::query::<AnomalyQuery>())
        .and(database.clone())
        .and_then(|query: AnomalyQuery, db: Arc<Database>| async move {
            let (limit, offset) = pagination(query.limit, query.offset)?;
            response(
                db.anomalies(
                    query.connection_id.as_deref(),
                    query.reason.as_deref(),
                    limit,
                    offset,
                )
                .await,
            )
        });
    let gaps = warp::path!("api" / "v1" / "collection-gaps")
        .and(warp::get())
        .and(auth)
        .and(warp::query::<RangeQuery>())
        .and(database)
        .and_then(|query: RangeQuery, db: Arc<Database>| async move {
            let (from, to) = range(query.from.as_deref(), query.to.as_deref())?;
            let timezone = FixedOffset::east_opt(8 * 3600).unwrap();
            let from = timezone
                .from_local_datetime(&from.and_hms_opt(0, 0, 0).unwrap())
                .unwrap()
                .timestamp_millis();
            let to = timezone
                .from_local_datetime(&to.and_hms_opt(0, 0, 0).unwrap())
                .unwrap()
                .timestamp_millis();
            response(db.gaps(from, to).await.map(|rows| json!(rows)))
        });
    health
        .or(stats)
        .or(connections)
        .or(domains)
        .or(anomalies)
        .or(gaps)
        .boxed()
        .recover(recover)
}

async fn stats(
    query: StatsQuery,
    db: Arc<Database>,
) -> std::result::Result<warp::reply::Json, Rejection> {
    let dimension = query.dimension.as_deref().unwrap_or("global");
    let period = query.period.as_deref().unwrap_or("day");
    if !["global", "airport", "group"].contains(&dimension) {
        return Err(bad_request("dimension 必须为 global、airport 或 group"));
    }
    if !["day", "month"].contains(&period) {
        return Err(bad_request("period 必须为 day 或 month"));
    }
    if dimension == "global" && query.name.is_some() {
        return Err(bad_request("global 维度不接受 name"));
    }
    let (from, to) = range(query.from.as_deref(), query.to.as_deref())?;
    let rows = db
        .stats(
            dimension,
            period,
            &from.to_string(),
            &to.to_string(),
            query.name.as_deref(),
        )
        .await;
    response(rows.map(|rows| {
        json!({"dimension": dimension, "period": period, "from": from.to_string(),
        "to": to.to_string(), "timezone": "Asia/Shanghai", "items": rows})
    }))
}

async fn recover(rejection: Rejection) -> std::result::Result<impl Reply, Infallible> {
    let (status, message) = if let Some(error) = rejection.find::<ApiError>() {
        match error {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message.clone()),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "认证失败".to_owned()),
            ApiError::Database => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "数据库查询失败".to_owned(),
            ),
        }
    } else if rejection.is_not_found() {
        (StatusCode::NOT_FOUND, "接口不存在".to_owned())
    } else if rejection.find::<warp::reject::MethodNotAllowed>().is_some() {
        (
            StatusCode::METHOD_NOT_ALLOWED,
            "不支持该请求方法".to_owned(),
        )
    } else if rejection.find::<warp::reject::InvalidQuery>().is_some()
        || rejection.find::<warp::reject::InvalidHeader>().is_some()
    {
        (StatusCode::BAD_REQUEST, "请求参数或请求头无效".to_owned())
    } else {
        eprintln!("API 请求失败：{rejection:?}");
        (StatusCode::INTERNAL_SERVER_ERROR, "请求处理失败".to_owned())
    };
    Ok(warp::reply::with_status(
        warp::reply::json(&json!({"status": "error", "data": null, "message": message})),
        status,
    ))
}
