use std::{error::Error, net::SocketAddr};

use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("yorm_api=info")),
        )
        .init();

    let address: SocketAddr = std::env::var("YORM_API_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse()
        .expect("YORM_API_ADDR must be a valid socket address");
    let database_url = std::env::var("DATABASE_URL").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "DATABASE_URL is required for the Yorm Pay sandbox API",
        )
    })?;

    let application = yorm_api::app_with_database(&database_url).await?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(%address, identity_store = "postgres", "Yorm Pay sandbox API listening");

    axum::serve(listener, application)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
