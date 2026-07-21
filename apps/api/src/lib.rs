mod error;
mod model;
mod store;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::{
    error::ApiError,
    model::{
        CreateIdentityRequest, CreateSessionRequest, IdentityView, PayLimitsResponse, PinRequest,
        PinVerificationResponse, SessionResponse,
    },
    store::{AuthenticatedIdentity, SandboxStore, epoch_seconds},
};

#[derive(Clone)]
struct AppState {
    status: Arc<SystemStatus>,
    store: Arc<SandboxStore>,
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
        store: Arc::new(SandboxStore::default()),
    };

    Router::new()
        .route("/health", get(health))
        .route("/v1/system/status", get(system_status))
        .route("/v1/sandbox/identities", post(create_identity))
        .route("/v1/sandbox/sessions", post(create_session))
        .route("/v1/me", get(get_me))
        .route("/v1/me/pin", put(set_pin))
        .route("/v1/me/pin/verify", post(verify_pin))
        .route("/v1/me/limits", get(get_limits))
        .route("/v1/me/session", delete(delete_session))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

async fn system_status(State(state): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json((*state.status).clone()))
}

async fn create_identity(
    State(state): State<AppState>,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<IdentityView>), ApiError> {
    let identity = state.store.register_identity(
        &request.email,
        &request.display_name,
        &request.country_code,
        epoch_seconds(),
    )?;

    Ok((StatusCode::CREATED, Json(identity)))
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), ApiError> {
    let session = state
        .store
        .create_session(request.identity_id, epoch_seconds())?;

    Ok((StatusCode::CREATED, Json(session)))
}

async fn get_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IdentityView>, ApiError> {
    let identity = authenticate(&headers, &state)?;
    Ok(Json(identity.view))
}

async fn set_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PinRequest>,
) -> Result<StatusCode, ApiError> {
    let identity = authenticate(&headers, &state)?;
    state.store.set_pin(identity.id, &request.pin)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn verify_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PinRequest>,
) -> Result<Json<PinVerificationResponse>, ApiError> {
    let identity = authenticate(&headers, &state)?;
    let result = state
        .store
        .verify_pin(identity.id, &request.pin, epoch_seconds())?;
    Ok(Json(result))
}

async fn get_limits(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PayLimitsResponse>, ApiError> {
    let identity = authenticate(&headers, &state)?;
    Ok(Json(SandboxStore::limits_for_country(
        &identity.view.country_code,
    )))
}

async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let token = bearer_token(&headers)?;
    state.store.revoke_session(token)?;
    Ok(StatusCode::NO_CONTENT)
}

fn authenticate(headers: &HeaderMap, state: &AppState) -> Result<AuthenticatedIdentity, ApiError> {
    state.store.authenticate(bearer_token(headers)?, epoch_seconds())
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or_else(|| {
            ApiError::unauthorized("SESSION_REQUIRED", "Authorization header is required")
        })?;
    let value = header.to_str().map_err(|_| {
        ApiError::unauthorized("SESSION_INVALID", "Authorization header is invalid")
    })?;
    let token = value.strip_prefix("Bearer ").ok_or_else(|| {
        ApiError::unauthorized(
            "SESSION_INVALID",
            "Authorization must use the Bearer scheme",
        )
    })?;

    if token.is_empty() {
        return Err(ApiError::unauthorized(
            "SESSION_INVALID",
            "Bearer token is empty",
        ));
    }

    Ok(token)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{app, sandbox_status};

    #[test]
    fn sandbox_disables_real_money_and_external_providers() {
        let status = sandbox_status();

        assert!(!status.real_money_enabled);
        assert!(!status.external_providers_enabled);
        assert_eq!(status.environment, "sandbox");
    }

    #[tokio::test]
    async fn protected_route_rejects_missing_session() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/v1/me")
                    .body(Body::empty())
                    .expect("request should be valid"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_route_rejects_invalid_session() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/v1/me")
                    .header("Authorization", "Bearer invalid-token")
                    .body(Body::empty())
                    .expect("request should be valid"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
