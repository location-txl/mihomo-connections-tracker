mod api;
mod collector;
mod common;
mod config;
mod db;
mod service;
#[cfg(test)]
mod tests;

use common::Result;
use config::Invocation;
use db::Database;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    let config = match config::parse_args(std::env::args_os()).unwrap_or_else(|error| error.exit())
    {
        Invocation::Run(config) => config,
        Invocation::Service(command) => return service::run(command),
    };
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let shutdown_signal = async {
        #[cfg(unix)]
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal,
            _ = terminate.recv() => Ok(()),
        }
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await
    };
    let db = Arc::new(Database::open(&config.database).await?);
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(config.listen_host, config.listen_port))
            .await?;
    println!(
        "查询 API：http://{}/api/v1，数据库：{}",
        listener.local_addr()?,
        config.database
    );
    let routes = api::routes(db.clone(), config.api_token.clone());
    let (shutdown, receiver) = watch::channel(false);
    let result = {
        let collection = collector::run(config, db.clone(), receiver.clone());
        let mut api_shutdown = receiver;
        let server = warp::serve(routes)
            .incoming(listener)
            .graceful(async move {
                let _ = api_shutdown.changed().await;
            })
            .run();
        tokio::pin!(collection, server);
        tokio::select! {
            result = &mut collection => result,
            _ = &mut server => Err("查询 API 意外停止".into()),
            signal = shutdown_signal => {
                signal?;
                let _ = shutdown.send(true);
                let result = collection.await;
                server.await;
                result
            }
        }
    };
    db.pool.close().await;
    result
}
