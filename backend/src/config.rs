use clap::Parser;

#[derive(Parser, Clone)]
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
