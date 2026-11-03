mod api;
mod collector;
mod common;
mod config;
mod db;
#[cfg(test)]
mod tests;

use clap::Parser;
use common::Result;
use config::Config;
use db::Database;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::parse();
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
            signal = tokio::signal::ctrl_c() => {
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
