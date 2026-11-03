use crate::{
    api,
    common::{default_database_path, Result},
    config::WebArgs,
    db::Database,
};
use rust_embed::RustEmbed;
use std::{net::SocketAddr, sync::Arc};
use warp::{Filter, Reply};

/// 前端构建产物，编译时嵌入二进制；先在 frontend 目录执行 pnpm build。
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../frontend/dist"]
struct Assets;

/// 前台运行 Web 控制台：同端口提供查询 API 与前端页面，Ctrl+C 退出。
pub async fn run(args: WebArgs) -> Result<()> {
    let path = match &args.database {
        Some(path) => path.clone(),
        None => default_database_path()?,
    };
    if !std::path::Path::new(&path).exists() {
        return Err(format!("数据库不存在：{path}").into());
    }
    let db = Arc::new(Database::open(&path).await?);
    let routes = api::endpoints(db, args.api_token)
        .or(static_site())
        .recover(api::recover);
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(args.listen_host, args.listen_port)).await?;
    println!(
        "Web 控制台：http://{}/，数据库：{}（Ctrl+C 退出）",
        listener.local_addr()?,
        path
    );
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let shutdown_signal = async move {
        #[cfg(unix)]
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal,
            _ = terminate.recv() => Ok(()),
        }
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await
    };
    let server = warp::serve(routes)
        .incoming(listener)
        .graceful(async move {
            let _ = shutdown_signal.await;
        })
        .run();
    server.await;
    Ok(())
}

fn static_site() -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    warp::get()
        .and(warp::path::full())
        .and_then(|path: warp::path::FullPath| async move {
            // API 路径不落入静态资源，保留 401/404 等原始错误。
            if path.as_str().starts_with("/api/") {
                Err(warp::reject::not_found())
            } else {
                Ok(path)
            }
        })
        .map(|path: warp::path::FullPath| asset_reply(path.as_str()))
}

fn asset_reply(path: &str) -> warp::reply::Response {
    let relative = path.trim_start_matches('/');
    let name = if !relative.is_empty() && Assets::get(relative).is_some() {
        relative
    } else {
        // SPA：未知路径回退到入口页。
        "index.html"
    };
    let Some(file) = Assets::get(name) else {
        return warp::reply::with_status(
            "前端资源未构建：在 frontend 目录执行 pnpm build 后重新编译",
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response();
    };
    let cache = if relative.starts_with("assets/") {
        // Vite 产物文件名带内容哈希，可长期缓存。
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    warp::http::Response::builder()
        .header(
            "Content-Type",
            mime_guess::from_path(name).first_or_octet_stream().as_ref(),
        )
        .header("Cache-Control", cache)
        .body(file.data.into_owned())
        .into_response()
}
