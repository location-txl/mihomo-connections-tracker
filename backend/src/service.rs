use crate::{common::Result, config::ServiceCommand};

pub fn run(command: ServiceCommand) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::run(command);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = command;
        Err("service 子命令仅支持 macOS".into())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::config::Config;
    use serde_json::{json, Value};
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        path::{Path, PathBuf},
        process::{Command, Output, Stdio},
        time::{Duration, Instant},
    };

    const LABEL: &str = "local.mihomo-connections-tracker";
    const BINARY: &str = "mihomo-connections-tracker";

    struct Service {
        base: PathBuf,
        agent: PathBuf,
        logs: PathBuf,
        domain: String,
    }

    impl Service {
        fn new() -> Result<Self> {
            let home = PathBuf::from(std::env::var_os("HOME").ok_or("未设置 HOME")?);
            let uid = Command::new("/usr/bin/id").arg("-u").output()?;
            let uid = checked(uid, "获取用户 ID")?;
            Ok(Self {
                base: home.join("Library/Application Support").join(BINARY),
                agent: home
                    .join("Library/LaunchAgents")
                    .join(format!("{LABEL}.plist")),
                logs: home.join("Library/Logs").join(BINARY),
                domain: format!("gui/{}", uid.trim()),
            })
        }

        fn config(&self) -> PathBuf {
            self.base.join("service.plist")
        }

        fn target(&self) -> String {
            format!("{}/{LABEL}", self.domain)
        }

        fn launchctl(&self, args: &[&str]) -> Result<Output> {
            Ok(Command::new("/bin/launchctl").args(args).output()?)
        }

        fn loaded(&self) -> Result<bool> {
            let output = self.launchctl(&["print", &self.target()])?;
            if output.status.success() {
                return Ok(true);
            }
            // launchctl 的 ESRCH 表示服务未注册；其他错误必须向上传播。
            if output.status.code() == Some(113) {
                return Ok(false);
            }
            checked(output, "查询服务")?;
            unreachable!()
        }

        fn stop(&self) -> Result<()> {
            if self.loaded()? {
                checked(self.launchctl(&["bootout", &self.target()])?, "停止服务")?;
                // bootout 返回时移除可能仍在进行，必须等待后才能重新 bootstrap。
                let deadline = Instant::now() + Duration::from_secs(35);
                while self.loaded()? {
                    if Instant::now() >= deadline {
                        return Err("等待服务卸载超时，请用 service status 检查状态".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(())
        }

        fn start(&self) -> Result<()> {
            if !self.config().is_file() {
                return Err("服务尚未安装，请先执行 service install".into());
            }
            if !self.loaded()? {
                checked(self.launchctl(&["enable", &self.target()])?, "启用服务")?;
                checked(
                    self.launchctl(&[
                        "bootstrap",
                        &self.domain,
                        self.config().to_str().ok_or("配置路径不是 UTF-8")?,
                    ])?,
                    "加载服务",
                )?;
            }
            Ok(())
        }

        fn enable(&self) -> Result<()> {
            let data = fs::read(self.config())?;
            fs::create_dir_all(self.agent.parent().unwrap())?;
            write_private(&self.agent, &data)?;
            self.start()
        }

        fn install(&self, config: Config, default_database: bool) -> Result<()> {
            let database = if default_database {
                self.base.join("tracker.db")
            } else {
                std::env::current_dir()?.join(&config.database)
            };
            let binary = std::env::current_exe()?;
            let definition = definition(&config, &binary, &database, &self.logs)?;
            let data = plist(&definition)?;
            self.stop()?;
            fs::create_dir_all(&self.base)?;
            fs::create_dir_all(&self.logs)?;
            write_private(&self.config(), &data)?;
            self.enable()?;
            println!("已安装并开启登录自启；使用 service status 查看进程状态");
            Ok(())
        }

        fn status(&self) -> Result<()> {
            println!("已安装：{}", self.config().is_file());
            println!("登录自启：{}", self.agent.is_file());
            println!("配置：{}", self.config().display());
            if self.config().is_file() {
                let output = Command::new("/usr/bin/plutil")
                    .args(["-extract", "ProgramArguments.0", "raw", "-o", "-"])
                    .arg(self.config())
                    .output()?;
                println!(
                    "可执行文件：{}",
                    checked(output, "读取服务程序路径")?.trim()
                );
            }
            println!("日志：{}", self.logs.display());
            if self.loaded()? {
                let output = checked(self.launchctl(&["print", &self.target()])?, "查询服务")?;
                // 完整输出包含环境变量中的密钥，只展示运行状态。
                for line in output.lines() {
                    let line = line.trim();
                    if line.starts_with("state =")
                        || line.starts_with("pid =")
                        || line.starts_with("last exit code =")
                    {
                        println!("{line}");
                    }
                }
            } else {
                println!("服务未加载");
            }
            Ok(())
        }
    }

    fn checked(output: Output, action: &str) -> Result<String> {
        if !output.status.success() {
            return Err(format!(
                "{action}失败（{}）：{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    fn write_private(path: &Path, data: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(data)?;
        Ok(())
    }

    fn remove_if_present(path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn definition(config: &Config, binary: &Path, database: &Path, logs: &Path) -> Result<Value> {
        let mut config = config.clone();
        config.database = database.to_string_lossy().into_owned();
        Ok(json!({
            "Label": LABEL,
            "ProgramArguments": [binary.to_string_lossy().into_owned(), "run"],
            "EnvironmentVariables": {
                "TRACKER_SERVICE_CONFIG": serde_json::to_string(&config)?
            },
            "RunAtLoad": true,
            "KeepAlive": true,
            "ThrottleInterval": 10,
            "ExitTimeOut": 30,
            "StandardOutPath": logs.join("stdout.log").to_string_lossy(),
            "StandardErrorPath": logs.join("stderr.log").to_string_lossy(),
        }))
    }

    fn plist(value: &Value) -> Result<Vec<u8>> {
        let mut child = Command::new("/usr/bin/plutil")
            .args(["-convert", "xml1", "-o", "-", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input_result = child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(value)?);
        let output = child.wait_with_output()?;
        input_result?;
        Ok(checked(output, "生成服务配置")?.into_bytes())
    }

    pub fn run(command: ServiceCommand) -> Result<()> {
        let service = Service::new()?;
        match command {
            ServiceCommand::Install {
                config,
                default_database,
            } => service.install(config, default_database)?,
            ServiceCommand::Start => service.start()?,
            ServiceCommand::Stop => service.stop()?,
            ServiceCommand::Restart => {
                service.stop()?;
                service.start()?;
            }
            ServiceCommand::Status => service.status()?,
            ServiceCommand::Enable => service.enable()?,
            ServiceCommand::Disable => {
                service.stop()?;
                remove_if_present(&service.agent)?;
            }
            ServiceCommand::Uninstall => {
                service.stop()?;
                remove_if_present(&service.agent)?;
                remove_if_present(&service.config())?;
                println!("服务已卸载，命令行程序、数据库和日志已保留");
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use clap::Parser;

        #[test]
        fn plist_roundtrip_preserves_arguments_and_secrets() {
            let mut config =
                Config::try_parse_from(["tracker", "--main-airport=-机场 <A> & B"]).unwrap();
            config.mihomo_token = Some("a<&>\"'\\\n密钥".into());
            config.api_token = Some("api&token".into());
            let value = definition(
                &config,
                Path::new("/tmp/app support/bin/tracker"),
                Path::new("/tmp/data space/tracker.db"),
                Path::new("/tmp/logs"),
            )
            .unwrap();
            let bytes = plist(&value).unwrap();
            let mut child = Command::new("/usr/bin/plutil")
                .args(["-convert", "json", "-o", "-", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&bytes).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            let decoded: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(decoded, value);
            let arguments = decoded["ProgramArguments"].as_array().unwrap();
            assert_eq!(
                arguments,
                &vec![json!("/tmp/app support/bin/tracker"), json!("run")]
            );
            let parsed: Config = serde_json::from_str(
                decoded["EnvironmentVariables"]["TRACKER_SERVICE_CONFIG"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(parsed.mihomo_token, config.mihomo_token);
            assert_eq!(parsed.api_token, config.api_token);
            assert_eq!(parsed.main_airport, config.main_airport);
            assert_eq!(parsed.database, "/tmp/data space/tracker.db");
            assert!(!decoded["ProgramArguments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg == config.mihomo_token.as_ref().unwrap()));
        }
    }
}
