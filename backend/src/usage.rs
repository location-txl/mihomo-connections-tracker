use crate::{
    common::{default_database_path, local_day, month_start, Result},
    config::UsageArgs,
    db::Database,
};
use chrono::{Duration, Months, NaiveDate, Utc};
use serde_json::Value;
use std::collections::BTreeMap;

pub async fn run(args: UsageArgs) -> Result<()> {
    let range = resolve_range(&args, local_day(Utc::now()))?;
    let path = database_path(&args)?;
    if !std::path::Path::new(&path).exists() {
        return Err(format!("数据库不存在：{path}").into());
    }
    let (dimension, name) = match (&args.airport, &args.group) {
        (Some(_), None) => ("airport", non_empty(args.airport.as_deref())),
        (None, Some(_)) => ("group", non_empty(args.group.as_deref())),
        _ => ("global", None),
    };
    let db = Database::open(&path).await?;
    let rows = db
        .stats(
            dimension,
            "day",
            &range.from.to_string(),
            &range.to.to_string(),
            name,
        )
        .await?;
    println!("{}", render(&range, dimension, name, &rows));
    Ok(())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

fn database_path(args: &UsageArgs) -> Result<String> {
    match &args.database {
        Some(path) => Ok(path.clone()),
        None => default_database_path(),
    }
}

struct TimeRange {
    from: NaiveDate,
    to: NaiveDate,
    label: String,
}

fn resolve_range(args: &UsageArgs, today: NaiveDate) -> Result<TimeRange> {
    if let Some(day) = args.day.as_deref() {
        let date = if day.is_empty() {
            today
        } else {
            NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .map_err(|_| format!("日期格式应为 YYYY-MM-DD，得到「{day}」"))?
        };
        return Ok(TimeRange {
            label: format!("{date} "),
            from: date,
            to: date + Duration::days(1),
        });
    }
    if let Some(month) = args.month.as_deref() {
        let start = if month.is_empty() {
            month_start(today)
        } else {
            NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
                .map_err(|_| format!("月份格式应为 YYYY-MM，得到「{month}」"))?
        };
        return Ok(TimeRange {
            label: format!("{} ", start.format("%Y-%m")),
            from: start,
            to: start + Months::new(1),
        });
    }
    let days: u32 = match non_empty(args.last.as_deref()) {
        Some(text) => text
            .parse()
            .map_err(|_| format!("--last 需要正整数，得到「{text}」"))?,
        None => 7,
    };
    if days == 0 {
        return Err("--last 至少为 1 天".into());
    }
    let from = today - Duration::days(days as i64 - 1);
    Ok(TimeRange {
        label: format!("最近 {days} 天（{from} ~ {today}）"),
        from,
        to: today + Duration::days(1),
    })
}

#[derive(Default)]
struct Usage {
    upload: u64,
    download: u64,
}

impl Usage {
    fn total(&self) -> u64 {
        self.upload + self.download
    }

    fn add(&mut self, other: &Usage) {
        self.upload += other.upload;
        self.download += other.download;
    }
}

fn usage_of(row: &Value) -> Option<Usage> {
    Some(Usage {
        upload: row.get("upload")?.as_u64()?,
        download: row.get("download")?.as_u64()?,
    })
}

fn aggregate_by_name(rows: &[Value]) -> Vec<(String, Usage)> {
    let mut totals: BTreeMap<String, Usage> = BTreeMap::new();
    for row in rows {
        // name 为空的是 direct/reject 等未归属机场的流量，不属于任何机场。
        let Some(name) = row
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        if let Some(usage) = usage_of(row) {
            totals.entry(name.to_owned()).or_default().add(&usage);
        }
    }
    let mut entries: Vec<_> = totals.into_iter().collect();
    entries.sort_by(|a, b| b.1.total().cmp(&a.1.total()));
    entries
}

fn aggregate_by_day(rows: &[Value]) -> Vec<(String, Usage)> {
    let mut totals: BTreeMap<String, Usage> = BTreeMap::new();
    for row in rows {
        let Some(day) = row.get("period").and_then(Value::as_str) else {
            continue;
        };
        if let Some(usage) = usage_of(row) {
            totals.entry(day.to_owned()).or_default().add(&usage);
        }
    }
    totals.into_iter().collect()
}

fn render(range: &TimeRange, dimension: &str, name: Option<&str>, rows: &[Value]) -> String {
    let kind = if dimension == "airport" {
        "机场"
    } else {
        "代理组"
    };
    match name {
        None if dimension != "global" => {
            let entries = aggregate_by_name(rows);
            if entries.is_empty() {
                return "该时间段内没有数据".into();
            }
            table(&format!("{}各{kind}流量", range.label), kind, &entries)
        }
        Some(name) => {
            let days = aggregate_by_day(rows);
            if days.is_empty() {
                return "该时间段内没有数据".into();
            }
            let title = format!("{}{kind}「{name}」", range.label);
            if days.len() == 1 {
                let usage = &days[0].1;
                format!(
                    "{title}\n上行 {}   下行 {}   合计 {}",
                    format_bytes(usage.upload),
                    format_bytes(usage.download),
                    format_bytes(usage.total())
                )
            } else {
                table(&title, "日期", &days)
            }
        }
        None => {
            let days = aggregate_by_day(rows);
            if days.is_empty() {
                return "该时间段内没有数据".into();
            }
            table(&format!("{}全部", range.label), "日期", &days)
        }
    }
}

/// 输出四列表格（首列左对齐，数值列右对齐），超过一行时在末尾追加合计行。
fn table(title: &str, first_header: &str, rows: &[(String, Usage)]) -> String {
    let columns_of = |label: &str, usage: &Usage| {
        [
            label.to_owned(),
            format_bytes(usage.upload),
            format_bytes(usage.download),
            format_bytes(usage.total()),
        ]
    };
    let header = [
        first_header.to_owned(),
        "上行".into(),
        "下行".into(),
        "合计".into(),
    ];
    let mut body: Vec<[String; 4]> = rows
        .iter()
        .map(|(label, usage)| columns_of(label, usage))
        .collect();
    if rows.len() > 1 {
        let mut total = Usage::default();
        for (_, usage) in rows {
            total.add(usage);
        }
        body.push(columns_of("合计", &total));
    }
    let widths: [usize; 4] = std::array::from_fn(|i| {
        std::iter::once(&header)
            .chain(&body)
            .map(|row| display_width(&row[i]))
            .max()
            .unwrap_or(0)
    });
    let row_text = |row: &[String; 4]| -> String {
        row.iter()
            .enumerate()
            .map(|(i, cell)| {
                let padding = " ".repeat(widths[i] - display_width(cell));
                if i == 0 {
                    format!("{cell}{padding}")
                } else {
                    format!("{padding}{cell}")
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    let total_at = if rows.len() > 1 {
        body.len() - 1
    } else {
        usize::MAX
    };
    let mut lines = vec![title.to_owned(), row_text(&header)];
    for (index, row) in body.iter().enumerate() {
        if index == total_at {
            lines.push("─".repeat(widths.iter().sum::<usize>() + 6));
        }
        lines.push(row_text(row));
    }
    lines.join("\n")
}

/// 按 ASCII 字符宽 1、其他宽 2 的近似值计算终端显示宽度。
fn display_width(text: &str) -> usize {
    text.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde_json::json;

    fn usage_args(
        day: Option<&str>,
        month: Option<&str>,
        last: Option<&str>,
        airport: Option<&str>,
        group: Option<&str>,
    ) -> UsageArgs {
        UsageArgs {
            day: day.map(str::to_owned),
            month: month.map(str::to_owned),
            last: last.map(str::to_owned),
            airport: airport.map(str::to_owned),
            group: group.map(str::to_owned),
            database: None,
        }
    }

    #[test]
    fn format_bytes_uses_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024_u64.pow(4)), "1.0 TB");
        assert_eq!(format_bytes(3 * 1024_u64.pow(5)), "3.0 PB");
    }

    #[test]
    fn resolves_explicit_ranges() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let range = resolve_range(
            &usage_args(Some("2026-10-01"), None, None, None, None),
            today,
        )
        .unwrap();
        assert_eq!(range.from.to_string(), "2026-10-01");
        assert_eq!(range.to.to_string(), "2026-10-02");

        let range =
            resolve_range(&usage_args(None, Some("2026-10"), None, None, None), today).unwrap();
        assert_eq!(range.from.to_string(), "2026-10-01");
        assert_eq!(range.to.to_string(), "2026-11-01");

        let range = resolve_range(&usage_args(None, None, Some("3"), None, None), today).unwrap();
        assert_eq!(range.from.to_string(), "2026-11-01");
        assert_eq!(range.to.to_string(), "2026-11-04");
    }

    #[test]
    fn resolves_default_ranges() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let range = resolve_range(&usage_args(Some(""), None, None, None, None), today).unwrap();
        assert_eq!(range.from, today);
        assert_eq!(range.to.to_string(), "2026-11-04");

        let range = resolve_range(&usage_args(None, Some(""), None, None, None), today).unwrap();
        assert_eq!(range.from.to_string(), "2026-11-01");
        assert_eq!(range.to.to_string(), "2026-12-01");

        // 不传时间参数默认最近 7 天。
        let range = resolve_range(&usage_args(None, None, None, None, None), today).unwrap();
        assert_eq!(range.from.to_string(), "2026-10-28");
        assert_eq!(range.to.to_string(), "2026-11-04");

        // --last 显式传 7 与默认结果一致。
        let range = resolve_range(&usage_args(None, None, Some("7"), None, None), today).unwrap();
        assert_eq!(range.from.to_string(), "2026-10-28");

        assert!(resolve_range(&usage_args(None, None, Some("0"), None, None), today).is_err());
        assert!(resolve_range(
            &usage_args(Some("2026/10/01"), None, None, None, None),
            today
        )
        .is_err());
        assert!(
            resolve_range(&usage_args(None, Some("2026-13"), None, None, None), today).is_err()
        );
    }

    fn range_2026_11() -> TimeRange {
        TimeRange {
            from: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
            label: "最近 3 天（2026-11-01 ~ 2026-11-03）".into(),
        }
    }

    #[test]
    fn aggregates_and_sorts_airport_list() {
        let rows = vec![
            json!({"period": "2026-11-01", "name": "甲机场", "upload": 100, "download": 200}),
            json!({"period": "2026-11-02", "name": "甲机场", "upload": 50, "download": 0}),
            json!({"period": "2026-11-01", "name": "乙机场", "upload": 1000, "download": 2000}),
            json!({"period": "2026-11-01", "name": "", "upload": 99, "download": 99}),
        ];
        let text = render(&range_2026_11(), "airport", None, &rows);
        assert!(text.contains("各机场流量"));
        // 乙机场总量更大排在前面，甲机场两天流量被合并。
        assert!(text.find("乙机场").unwrap() < text.find("甲机场").unwrap());
        assert!(text.contains("150 B"));
        assert!(text.contains("350 B"));
        assert!(!text.contains("99 B"));
        assert!(text.contains("合计"));
    }

    #[test]
    fn renders_single_day_named_summary() {
        let rows =
            vec![json!({"period": "2026-11-03", "name": null, "upload": 1024, "download": 2048})];
        let text = render(&range_2026_11(), "airport", Some("甲机场"), &rows);
        assert!(text.contains("机场「甲机场」"));
        assert!(text.contains("上行 1.0 KB   下行 2.0 KB   合计 3.0 KB"));
        assert!(!text.contains('─'));
    }

    #[test]
    fn renders_daily_table_with_total() {
        let rows = vec![
            json!({"period": "2026-11-01", "name": null, "upload": 1024, "download": 0}),
            json!({"period": "2026-11-02", "name": null, "upload": 2048, "download": 1024}),
        ];
        let text = render(&range_2026_11(), "global", None, &rows);
        assert!(text.contains("全部"));
        assert!(text.contains("2026-11-01"));
        assert!(text.contains("合计"));
        assert!(text.contains("3.0 KB"));
    }

    #[test]
    fn reports_empty_data() {
        assert_eq!(
            render(&range_2026_11(), "global", None, &[]),
            "该时间段内没有数据"
        );
    }
}
