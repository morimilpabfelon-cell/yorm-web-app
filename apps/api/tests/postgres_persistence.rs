use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn identity_session_pin_lock_and_revocation_survive_router_recreation() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured for PostgreSQL integration tests");
    let database_pool = PgPool::connect(&database_url)
        .await
        .expect("PostgreSQL should accept connections");

    let email = format!("persist-{}@yorm.local", Uuid::new_v4());
    let lock_email = format!("lock-{}@yorm.local", Uuid::new_v4());
    let first_app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should initialize with PostgreSQL");

    let database_health = request(&first_app, Method::GET, "/health/database", None, None).await;
    assert_eq!(database_health.status(), StatusCode::OK);
    let database_health_body = json_body(database_health).await;
    assert_eq!(database_health_body["backend"], "postgres");

    let identity_response = request(
        &first_app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": email,
            "display_name": "Persistent Sandbox",
            "country_code": "PE"
        })),
        None,
    )
    .await;
    assert_eq!(identity_response.status(), StatusCode::CREATED);
    let identity_body = json_body(identity_response).await;
    let identity_id = Uuid::parse_str(
        identity_body["id"]
            .as_str()
            .expect("identity response should contain an id"),
    )
    .expect("identity id should be a UUID");

    let duplicate_identity = request(
        &first_app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": identity_body["email"],
            "display_name": "Duplicate Sandbox",
            "country_code": "PE"
        })),
        None,
    )
    .await;
    assert_eq!(duplicate_identity.status(), StatusCode::CONFLICT);

    let session_response = request(
        &first_app,
        Method::POST,
        "/v1/sandbox/sessions",
        Some(json!({ "identity_id": identity_id })),
        None,
    )
    .await;
    assert_eq!(session_response.status(), StatusCode::CREATED);
    let session_body = json_body(session_response).await;
    let access_token = session_body["access_token"]
        .as_str()
        .expect("session response should contain an access token")
        .to_owned();

    let set_pin_response = request(
        &first_app,
        Method::PUT,
        "/v1/me/pin",
        Some(json!({ "pin": "4096" })),
        Some(&access_token),
    )
    .await;
    assert_eq!(set_pin_response.status(), StatusCode::NO_CONTENT);

    let stored_pin_hash: String =
        sqlx::query_scalar("SELECT pin_hash FROM sandbox_identities WHERE id = $1")
            .bind(identity_id)
            .fetch_one(&database_pool)
            .await
            .expect("PIN hash should be persisted");
    assert_ne!(stored_pin_hash, "4096");
    assert!(stored_pin_hash.starts_with("$argon2"));

    let stored_token_digest: String =
        sqlx::query_scalar("SELECT token_digest FROM sandbox_sessions WHERE identity_id = $1")
            .bind(identity_id)
            .fetch_one(&database_pool)
            .await
            .expect("session digest should be persisted");
    assert_ne!(stored_token_digest, access_token);
    assert!(stored_token_digest.len() >= 40);

    drop(first_app);

    let second_app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should reconnect to the same PostgreSQL database");
    let persisted_profile = request(
        &second_app,
        Method::GET,
        "/v1/me",
        None,
        Some(&access_token),
    )
    .await;
    assert_eq!(persisted_profile.status(), StatusCode::OK);
    let persisted_profile_body = json_body(persisted_profile).await;
    assert_eq!(persisted_profile_body["id"], identity_id.to_string());
    assert_eq!(persisted_profile_body["pin_configured"], true);

    let persisted_pin = request(
        &second_app,
        Method::POST,
        "/v1/me/pin/verify",
        Some(json!({ "pin": "4096" })),
        Some(&access_token),
    )
    .await;
    assert_eq!(persisted_pin.status(), StatusCode::OK);

    let lock_identity_response = request(
        &second_app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": lock_email,
            "display_name": "Persistent Lock",
            "country_code": "PE"
        })),
        None,
    )
    .await;
    assert_eq!(lock_identity_response.status(), StatusCode::CREATED);
    let lock_identity_body = json_body(lock_identity_response).await;
    let lock_identity_id = Uuid::parse_str(
        lock_identity_body["id"]
            .as_str()
            .expect("lock identity response should contain an id"),
    )
    .expect("lock identity id should be a UUID");

    let lock_session_response = request(
        &second_app,
        Method::POST,
        "/v1/sandbox/sessions",
        Some(json!({ "identity_id": lock_identity_id })),
        None,
    )
    .await;
    let lock_session_body = json_body(lock_session_response).await;
    let lock_access_token = lock_session_body["access_token"]
        .as_str()
        .expect("lock session should contain an access token")
        .to_owned();

    let lock_set_pin = request(
        &second_app,
        Method::PUT,
        "/v1/me/pin",
        Some(json!({ "pin": "4096" })),
        Some(&lock_access_token),
    )
    .await;
    assert_eq!(lock_set_pin.status(), StatusCode::NO_CONTENT);

    for attempt in 1..=5 {
        let incorrect_pin = request(
            &second_app,
            Method::POST,
            "/v1/me/pin/verify",
            Some(json!({ "pin": "9876" })),
            Some(&lock_access_token),
        )
        .await;
        let expected_status = if attempt < 5 {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::from_u16(423).expect("423 should be a valid status")
        };
        assert_eq!(incorrect_pin.status(), expected_status);
    }

    drop(second_app);

    let third_app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should reconnect after PIN lock");
    let persisted_lock = request(
        &third_app,
        Method::POST,
        "/v1/me/pin/verify",
        Some(json!({ "pin": "4096" })),
        Some(&lock_access_token),
    )
    .await;
    assert_eq!(
        persisted_lock.status(),
        StatusCode::from_u16(423).expect("423 should be a valid status")
    );

    let logout = request(
        &third_app,
        Method::DELETE,
        "/v1/me/session",
        None,
        Some(&access_token),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    drop(third_app);

    let fourth_app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should reconnect after logout");
    let revoked_session = request(
        &fourth_app,
        Method::GET,
        "/v1/me",
        None,
        Some(&access_token),
    )
    .await;
    assert_eq!(revoked_session.status(), StatusCode::UNAUTHORIZED);

    sqlx::query("DELETE FROM sandbox_identities WHERE id = ANY($1)")
        .bind(vec![identity_id, lock_identity_id])
        .execute(&database_pool)
        .await
        .expect("integration test identities should be removed");
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
