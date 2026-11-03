use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser, Clone, serde::Serialize, serde::Deserialize)]
#[command(version, about = "采集 Mihomo 连接并提供机场、代理组流量统计")]
pub struct Config {
    #[arg(long, default_value = "tracker.db")]
    pub database: String,
    #[arg(long, default_value = "127.0.0.1")]
    pub mihomo_host: String,
    #[arg(long, default_value_t = 9090)]
    pub mihomo_port: u16,
    #[arg(long, env = "MIHOMO_TOKEN", hide_env_values = true)]
    pub mihomo_token: Option<String>,
    /// 空 Provider 的代理节点所属机场名称。
    #[arg(long, value_parser = nonempty)]
    pub main_airport: String,
    #[arg(long, default_value = "127.0.0.1")]
    pub listen_host: std::net::IpAddr,
    #[arg(long, default_value_t = 8080)]
    pub listen_port: u16,
    #[arg(long, env = "TRACKER_API_TOKEN", hide_env_values = true)]
    pub api_token: Option<String>,
}

fn nonempty(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("机场名不能为空".into());
    }
    Ok(value.to_owned())
}

/// 查询流量统计。时间参数三选一，都不传默认最近 7 天。
#[derive(Parser)]
pub struct UsageArgs {
    /// 查询某一天（YYYY-MM-DD），不传值默认今天。
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        conflicts_with_all = ["month", "last"]
    )]
    pub day: Option<String>,
    /// 查询某个自然月（YYYY-MM），不传值默认当月。
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        conflicts_with_all = ["day", "last"]
    )]
    pub month: Option<String>,
    /// 查询最近 N 天（含今天），不传值默认 7。
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "7",
        conflicts_with_all = ["day", "month"]
    )]
    pub last: Option<String>,
    /// 按机场过滤，不传值列出所有机场。
    #[arg(long, num_args = 0..=1, default_missing_value = "", conflicts_with = "group")]
    pub airport: Option<String>,
    /// 按代理组过滤，不传值列出所有代理组。
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        conflicts_with = "airport"
    )]
    pub group: Option<String>,
    /// SQLite 文件，默认使用用户应用目录中的 tracker.db。
    #[arg(long)]
    pub database: Option<String>,
}

#[derive(Parser)]
#[command(name = "service", about = "管理 macOS 用户级后台服务")]
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

/// 启动 Web 控制台，前台运行，Ctrl+C 退出。
#[derive(Parser)]
pub struct WebArgs {
    /// HTTP 监听地址。
    #[arg(long, default_value = "127.0.0.1")]
    pub listen_host: std::net::IpAddr,
    /// HTTP 监听端口，默认 8081 以避开采集服务的查询 API。
    #[arg(long, default_value_t = 8081)]
    pub listen_port: u16,
    /// SQLite 文件，默认使用用户应用目录中的 tracker.db。
    #[arg(long)]
    pub database: Option<String>,
    /// 查询接口密钥，不设置则无需认证。
    #[arg(long, env = "TRACKER_API_TOKEN", hide_env_values = true)]
    pub api_token: Option<String>,
}

#[derive(Subcommand)]
pub enum ServiceCommand {
    /// 安装或更新服务，立即启动并开启登录自启。
    Install {
        #[command(flatten)]
        config: Config,
        #[arg(skip)]
        default_database: bool,
    },
    /// 启动当前会话，不改变登录自启设置。
    Start,
    /// 停止当前会话，不改变登录自启设置。
    Stop,
    /// 优雅停止后重新启动。
    Restart,
    /// 查看运行状态和文件路径。
    Status,
    /// 开启登录自启并立即启动。
    Enable,
    /// 关闭登录自启并停止。
    Disable,
    /// 卸载服务，保留数据库和日志。
    Uninstall,
}

#[derive(Parser)]
#[command(version, about = "采集 Mihomo 连接并提供机场、代理组流量统计")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 管理 macOS 用户级后台服务。
    Service(ServiceArgs),
    /// 查询流量统计。
    Usage(UsageArgs),
    /// 启动 Web 控制台，前台运行。
    Web(WebArgs),
    #[command(hide = true)]
    Run,
}

pub enum Invocation {
    Run,
    Service(ServiceCommand),
    Usage(UsageArgs),
    Web(WebArgs),
}

pub fn parse_args(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Invocation, clap::Error> {
    let service = ServiceArgs::command().mut_subcommand("install", |command| {
        command.mut_arg("database", |arg| {
            arg.hide_default_value(true)
                .help("SQLite 文件，默认使用用户应用目录中的 tracker.db")
        })
    });
    let matches = Cli::command()
        .mut_subcommand("service", |_| service)
        .try_get_matches_from(args)?;
    if let Some(service) = matches.subcommand_matches("service") {
        let mut command = ServiceArgs::from_arg_matches(service)?.command;
        if let ServiceCommand::Install {
            default_database, ..
        } = &mut command
        {
            let install = service.subcommand_matches("install").unwrap();
            *default_database =
                install.value_source("database") == Some(clap::parser::ValueSource::DefaultValue);
        }
        Ok(Invocation::Service(command))
    } else if let Some(usage) = matches.subcommand_matches("usage") {
        Ok(Invocation::Usage(UsageArgs::from_arg_matches(usage)?))
    } else if let Some(web) = matches.subcommand_matches("web") {
        Ok(Invocation::Web(WebArgs::from_arg_matches(web)?))
    } else {
        Ok(Invocation::Run)
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, clap::Error> {
        parse_args(args.iter().map(std::ffi::OsString::from))
    }

    #[test]
    fn foreground_and_service_arguments_are_separate() {
        for option in [
            "--database",
            "--mihomo-host",
            "--mihomo-port",
            "--mihomo-token",
            "--main-airport",
            "--listen-host",
            "--listen-port",
            "--api-token",
        ] {
            assert!(parse(&["tracker", option, "value"]).is_err());
            assert!(parse(&["tracker", "run", option, "value"]).is_err());
        }
        assert!(matches!(
            parse(&["tracker", "run"]).unwrap(),
            Invocation::Run
        ));
        // 顶层不接受配置参数；web 子命令的 --database 只在其自身生效。
        let help = Cli::command().render_help().to_string();
        assert!(!help.contains("--database"));
        assert!(!help.contains("--main-airport"));
        assert!(parse(&["tracker"]).is_err());
        for command in [
            "start",
            "stop",
            "restart",
            "status",
            "enable",
            "disable",
            "uninstall",
        ] {
            assert!(matches!(
                parse(&["tracker", "service", command]).unwrap(),
                Invocation::Service(_)
            ));
        }
        assert!(parse(&["tracker", "service", "install"]).is_err());
        assert!(parse(&["tracker", "--main-airport", "主机场", "service", "status"]).is_err());
    }

    #[test]
    fn installation_distinguishes_default_and_explicit_database() {
        for (args, expected) in [
            (
                vec!["tracker", "service", "install", "--main-airport", "主机场"],
                true,
            ),
            (
                vec![
                    "tracker",
                    "service",
                    "install",
                    "--main-airport",
                    "主机场",
                    "--database",
                    "tracker.db",
                ],
                false,
            ),
        ] {
            let Invocation::Service(ServiceCommand::Install {
                default_database, ..
            }) = parse(&args).unwrap()
            else {
                panic!("expected install");
            };
            assert_eq!(default_database, expected);
        }
    }

    #[test]
    fn usage_time_and_dimension_options_parse() {
        fn parse_usage(args: &[&str]) -> UsageArgs {
            let Invocation::Usage(usage) = parse(args).unwrap() else {
                panic!("expected usage");
            };
            usage
        }
        let usage = parse_usage(&["tracker", "usage"]);
        assert_eq!(usage.day, None);
        assert_eq!(usage.month, None);
        assert_eq!(usage.last, None);

        let usage = parse_usage(&["tracker", "usage", "--day", "2026-11-01"]);
        assert_eq!(usage.day.as_deref(), Some("2026-11-01"));
        let usage = parse_usage(&["tracker", "usage", "--day"]);
        assert_eq!(usage.day.as_deref(), Some(""));
        let usage = parse_usage(&["tracker", "usage", "--last", "30"]);
        assert_eq!(usage.last.as_deref(), Some("30"));
        let usage = parse_usage(&["tracker", "usage", "--last"]);
        assert_eq!(usage.last.as_deref(), Some("7"));
        let usage = parse_usage(&[
            "tracker",
            "usage",
            "--airport",
            "飞鸟",
            "--database",
            "x.db",
        ]);
        assert_eq!(usage.airport.as_deref(), Some("飞鸟"));
        assert_eq!(usage.database.as_deref(), Some("x.db"));
        let usage = parse_usage(&["tracker", "usage", "--group"]);
        assert_eq!(usage.group.as_deref(), Some(""));
    }

    #[test]
    fn usage_rejects_conflicting_options() {
        for args in [
            vec!["tracker", "usage", "--day", "--month"],
            vec!["tracker", "usage", "--day", "--last", "3"],
            vec!["tracker", "usage", "--month", "--last", "3"],
            vec![
                "tracker",
                "usage",
                "--airport",
                "飞鸟",
                "--group",
                "自动选择",
            ],
        ] {
            assert!(parse(&args).is_err(), "应互斥：{args:?}");
        }
    }

    #[test]
    fn web_options_parse() {
        let Invocation::Web(web) = parse(&[
            "tracker",
            "web",
            "--listen-port",
            "9000",
            "--database",
            "x.db",
        ])
        .unwrap() else {
            panic!("expected web");
        };
        assert_eq!(web.listen_port, 9000);
        assert_eq!(web.listen_host.to_string(), "127.0.0.1");
        assert_eq!(web.database.as_deref(), Some("x.db"));
        assert_eq!(web.api_token, None);

        let Invocation::Web(web) = parse(&["tracker", "web"]).unwrap() else {
            panic!("expected web");
        };
        assert_eq!(web.listen_port, 8081);
        assert_eq!(web.database, None);

        // web 不接受采集服务的配置参数。
        assert!(parse(&["tracker", "web", "--main-airport", "主机场"]).is_err());
        assert!(parse(&["tracker", "web", "--mihomo-port", "9090"]).is_err());
    }
}
