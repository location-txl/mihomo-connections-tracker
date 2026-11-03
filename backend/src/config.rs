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

#[derive(Parser)]
#[command(name = "service", about = "管理 macOS 用户级后台服务")]
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
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
    #[command(hide = true)]
    Run,
}

pub enum Invocation {
    Run,
    Service(ServiceCommand),
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
        let help = Cli::command().render_long_help().to_string();
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
}
