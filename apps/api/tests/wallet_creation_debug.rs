use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn sandbox_wallet_creation_returns_created() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured for PostgreSQL integration tests");
    let app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should initialize with PostgreSQL");

    let identity_response = request(
        &app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": format!("wallet-debug-{}@yorm.local", Uuid::new_v4()),
            "display_name": "Wallet Debug",
            "country_code": "PE"
        })),
        None,
    )
    .await;
    assert_eq!(identity_response.status(), StatusCode::CREATED);
    let identity = json_body(identity_response).await;

    let session_response = request(
        &app,
        Method::POST,
        "/v1/sandbox/sessions",
        Some(json!({ "identity_id": identity["id"] })),
        None,
    )
    .await;
    assert_eq!(session_response.status(), StatusCode::CREATED);
    let session = json_body(session_response).await;
    let access_token = session["access_token"]
        .as_str()
        .expect("session response should contain token");

    let wallet_response = request(
        &app,
        Method::POST,
        "/v1/me/wallet",
        None,
        Some(access_token),
    )
    .await;
    let status = wallet_response.status();
    let body = json_body(wallet_response).await;
    assert_eq!(status, StatusCode::CREATED, "wallet response: {body}");
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    access_token: Option<&str>,
) -> Response<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(token) = access_token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    app.clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map_or_else(String::new, |value| value.to_string()),
                ))
                .expect("test request should be valid"),
        )
        .await
        .expect("application should respond")
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).expect("response body should contain JSON")
}
