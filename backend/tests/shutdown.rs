#![cfg(unix)]

use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

#[tokio::test]
async fn termination_signals_close_database_cleanly() {
    for signal in ["-TERM", "-INT"] {
        let database: PathBuf = std::env::temp_dir().join(format!(
            "tracker-shutdown-{}-{signal}.db",
            std::process::id()
        ));
        let mut child = Command::new(env!("CARGO_BIN_EXE_mihomo-connections-tracker"))
            .args([
                "--main-airport",
                "测试机场",
                "--database",
                database.to_str().unwrap(),
                "--listen-port",
                "0",
            ])
            .env_remove("MIHOMO_TOKEN")
            .env_remove("TRACKER_API_TOKEN")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(stdout).lines();
        let ready = timeout(Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(ready.starts_with("查询 API："));
        assert!(Command::new("/bin/kill")
            .args([signal, &child.id().unwrap().to_string()])
            .status()
            .await
            .unwrap()
            .success());
        assert!(timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(&database),
        )
        .await
        .unwrap();
        let error: String =
            sqlx::query_scalar("SELECT last_error FROM collector_state WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(error, "service_stopped");
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(integrity, "ok");
        pool.close().await;
        std::fs::remove_file(database).unwrap();
    }
}
