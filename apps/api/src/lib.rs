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
        PinVerificationResponse, SandboxCreditRequest, SandboxCreditResponse,
        SandboxTransferRequest, SandboxTransferResponse, SessionResponse, WalletView,
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

#[derive(Debug, Serialize)]
struct DatabaseHealthResponse {
    status: &'static str,
    backend: &'static str,
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
    app_with_store(SandboxStore::default())
}

pub async fn app_with_database(database_url: &str) -> Result<Router, sqlx::Error> {
    Ok(app_with_store(
        SandboxStore::connect_postgres(database_url).await?,
    ))
}

fn app_with_store(store: SandboxStore) -> Router {
    let state = AppState {
        status: Arc::new(sandbox_status()),
        store: Arc::new(store),
    };

    Router::new()
        .route("/health", get(health))
        .route("/health/database", get(database_health))
        .route("/v1/system/status", get(system_status))
        .route("/v1/sandbox/identities", post(create_identity))
        .route("/v1/sandbox/sessions", post(create_session))
        .route("/v1/me", get(get_me))
        .route("/v1/me/pin", put(set_pin))
        .route("/v1/me/pin/verify", post(verify_pin))
        .route("/v1/me/limits", get(get_limits))
        .route("/v1/me/wallet", post(create_wallet).get(get_wallet))
        .route("/v1/sandbox/wallet/credits", post(credit_wallet))
        .route("/v1/sandbox/transfers", post(transfer_wallet))
        .route("/v1/me/session", delete(delete_session))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

async fn database_health(
    State(state): State<AppState>,
) -> Result<Json<DatabaseHealthResponse>, ApiError> {
    state.store.database_health().await?;
    Ok(Json(DatabaseHealthResponse {
        status: "ok",
        backend: state.store.backend_name(),
    }))
}

async fn system_status(State(state): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json((*state.status).clone()))
}

async fn create_identity(
    State(state): State<AppState>,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<IdentityView>), ApiError> {
    let identity = state
        .store
        .register_identity(
            &request.email,
            &request.display_name,
            &request.country_code,
            epoch_seconds(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(identity)))
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), ApiError> {
    let session = state
        .store
        .create_session(request.identity_id, epoch_seconds())
        .await?;

    Ok((StatusCode::CREATED, Json(session)))
}

async fn get_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IdentityView>, ApiError> {
    let identity = authenticate(&headers, &state).await?;
    Ok(Json(identity.view))
}

async fn set_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PinRequest>,
) -> Result<StatusCode, ApiError> {
    let identity = authenticate(&headers, &state).await?;
    state.store.set_pin(identity.id, &request.pin).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn verify_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PinRequest>,
) -> Result<Json<PinVerificationResponse>, ApiError> {
    let identity = authenticate(&headers, &state).await?;
    let result = state
        .store
        .verify_pin(identity.id, &request.pin, epoch_seconds())
        .await?;
    Ok(Json(result))
}

async fn get_limits(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PayLimitsResponse>, ApiError> {
    let identity = authenticate(&headers, &state).await?;
    Ok(Json(SandboxStore::limits_for_country(
        &identity.view.country_code,
    )))
}

async fn create_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalletView>), ApiError> {
    let identity = authenticate(&headers, &state).await?;
    let wallet = state
        .store
        .create_wallet(identity.id, epoch_seconds())
        .await?;
    Ok((StatusCode::CREATED, Json(wallet)))
}

async fn get_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletView>, ApiError> {
    let identity = authenticate(&headers, &state).await?;
    Ok(Json(state.store.get_wallet(identity.id).await?))
}

async fn credit_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SandboxCreditRequest>,
) -> Result<(StatusCode, Json<SandboxCreditResponse>), ApiError> {
    let identity = authenticate(&headers, &state).await?;
    if !identity.view.pin_configured {
        return Err(ApiError::conflict(
            "PIN_REQUIRED",
            "configure Pay Safe PIN before using sandbox wallet credits",
        ));
    }
    let key = idempotency_key(&headers)?;
    let credit = state
        .store
        .credit_wallet(
            identity.id,
            key,
            &request.amount_minor_units,
            epoch_seconds(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(credit)))
}

async fn transfer_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SandboxTransferRequest>,
) -> Result<(StatusCode, Json<SandboxTransferResponse>), ApiError> {
    let identity = authenticate(&headers, &state).await?;
    if !identity.view.pin_configured {
        return Err(ApiError::conflict(
            "PIN_REQUIRED",
            "configure Pay Safe PIN before using sandbox P2P transfers",
        ));
    }
    let key = idempotency_key(&headers)?;
    let transfer = state
        .store
        .transfer_wallet(
            identity.id,
            request.recipient_identity_id,
            key,
            &request.amount_minor_units,
            epoch_seconds(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(transfer)))
}

async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let token = bearer_token(&headers)?;
    state.store.revoke_session(token, epoch_seconds()).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn authenticate(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<AuthenticatedIdentity, ApiError> {
    state
        .store
        .authenticate(bearer_token(headers)?, epoch_seconds())
        .await
}

fn idempotency_key(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("idempotency-key")
        .ok_or_else(|| {
            ApiError::bad_request(
                "IDEMPOTENCY_KEY_REQUIRED",
                "Idempotency-Key header is required",
            )
        })?
        .to_str()
        .map_err(|_| {
            ApiError::bad_request(
                "IDEMPOTENCY_KEY_INVALID",
                "Idempotency-Key header must contain visible text",
            )
        })
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
