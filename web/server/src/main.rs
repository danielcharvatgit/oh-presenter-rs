use axum::{Router, routing::get};
use std::{env, net::SocketAddr, path::PathBuf};
use tower_http::services::{ServeDir, ServeFile};

async fn health() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() {
    let static_dir = env::var_os("OH_PRESENTER_WEB_DIST").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("../dist"));
    let fallback = ServeFile::new(static_dir.join("index.html"));
    let app = Router::new().route("/api/health", get(health)).fallback_service(ServeDir::new(static_dir).not_found_service(fallback));

    let address: SocketAddr = env::var("OH_PRESENTER_WEB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()
        .expect("OH_PRESENTER_WEB_ADDR must be a socket address");
    let listener = tokio::net::TcpListener::bind(address).await.expect("could not bind web server");
    println!("oh! Presenter web server listening on http://{address}");
    axum::serve(listener, app).await.expect("web server failed");
}
