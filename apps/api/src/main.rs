use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("yorm_api=info")),
        )
        .init();

    let address: SocketAddr = std::env::var("YORM_API_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse()
        .expect("YORM_API_ADDR must be a valid socket address");

    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(%address, "Yorm Pay sandbox API listening");

    axum::serve(listener, yorm_api::app())
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
