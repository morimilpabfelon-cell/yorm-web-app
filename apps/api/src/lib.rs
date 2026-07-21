use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    status: Arc<SystemStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemStatus {
    pub service: &'static str,
    pub environment: &'static str,
    pub version: &'static str,
    pub real_money_enabled: bool,
    pub external_providers_enabled: bool,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn sandbox_status() -> SystemStatus {
    SystemStatus {
        service: "yorm-api",
        environment: "sandbox",
        version: env!("CARGO_PKG_VERSION"),
        real_money_enabled: false,
        external_providers_enabled: false,
    }
}

pub fn app() -> Router {
    let state = AppState {
        status: Arc::new(sandbox_status()),
    };

    Router::new()
        .route("/health", get(health))
        .route("/v1/system/status", get(system_status))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthResponse { status: "ok" }),
    )
}

async fn system_status(State(state): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json((*state.status).clone()))
}

#[cfg(test)]
mod tests {
    use super::sandbox_status;

    #[test]
    fn sandbox_disables_real_money_and_external_providers() {
        let status = sandbox_status();

        assert!(!status.real_money_enabled);
        assert!(!status.external_providers_enabled);
        assert_eq!(status.environment, "sandbox");
    }
}
