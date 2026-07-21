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
async fn wallet_credit_is_balanced_immutable_and_idempotent() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured for PostgreSQL integration tests");
    let database_pool = PgPool::connect(&database_url)
        .await
        .expect("PostgreSQL should accept connections");
    let app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should initialize with PostgreSQL");

    let identity_response = request(
        &app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": format!("wallet-{}@yorm.local", Uuid::new_v4()),
            "display_name": "Wallet Ledger",
            "country_code": "PE"
        })),
        None,
        None,
    )
    .await;
    assert_eq!(identity_response.status(), StatusCode::CREATED);
    let identity = json_body(identity_response).await;
    let identity_id = Uuid::parse_str(
        identity["id"]
            .as_str()
            .expect("identity response should contain id"),
    )
    .expect("identity id should be a UUID");

    let session_response = request(
        &app,
        Method::POST,
        "/v1/sandbox/sessions",
        Some(json!({ "identity_id": identity_id })),
        None,
        None,
    )
    .await;
    assert_eq!(session_response.status(), StatusCode::CREATED);
    let session = json_body(session_response).await;
    let access_token = session["access_token"]
        .as_str()
        .expect("session response should contain token")
        .to_owned();

    let set_pin = request(
        &app,
        Method::PUT,
        "/v1/me/pin",
        Some(json!({ "pin": "4096" })),
        Some(&access_token),
        None,
    )
    .await;
    assert_eq!(set_pin.status(), StatusCode::NO_CONTENT);

    let create_wallet = request(
        &app,
        Method::POST,
        "/v1/me/wallet",
        None,
        Some(&access_token),
        None,
    )
    .await;
    assert_eq!(create_wallet.status(), StatusCode::CREATED);
    let wallet = json_body(create_wallet).await;
    let wallet_id = Uuid::parse_str(
        wallet["id"]
            .as_str()
            .expect("wallet response should contain id"),
    )
    .expect("wallet id should be a UUID");
    assert_eq!(wallet["identity_id"], identity_id.to_string());
    assert_eq!(wallet["currency"], "PEN");
    assert_eq!(wallet["balance_minor_units"], "0");

    let missing_key = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "1250" })),
        Some(&access_token),
        None,
    )
    .await;
    assert_eq!(missing_key.status(), StatusCode::BAD_REQUEST);

    let idempotency_key = format!("wallet-credit-{}", Uuid::new_v4());
    let credit = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "1250" })),
        Some(&access_token),
        Some(&idempotency_key),
    )
    .await;
    assert_eq!(credit.status(), StatusCode::CREATED);
    let credit_body = json_body(credit).await;
    let transaction_id = Uuid::parse_str(
        credit_body["transaction_id"]
            .as_str()
            .expect("credit response should contain transaction id"),
    )
    .expect("transaction id should be a UUID");
    assert_eq!(credit_body["wallet_id"], wallet_id.to_string());
    assert_eq!(credit_body["transaction_kind"], "sandbox_credit");
    assert_eq!(credit_body["currency"], "PEN");
    assert_eq!(credit_body["amount_minor_units"], "1250");
    assert_eq!(credit_body["balance_after_minor_units"], "1250");

    let replay = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "1250" })),
        Some(&access_token),
        Some(&idempotency_key),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::CREATED);
    let replay_body = json_body(replay).await;
    assert_eq!(replay_body["transaction_id"], transaction_id.to_string());
    assert_eq!(replay_body["balance_after_minor_units"], "1250");

    let conflicting_replay = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "1300" })),
        Some(&access_token),
        Some(&idempotency_key),
    )
    .await;
    assert_eq!(conflicting_replay.status(), StatusCode::CONFLICT);

    let limit_exceeded = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "100001" })),
        Some(&access_token),
        Some(&format!("wallet-limit-{}", Uuid::new_v4())),
    )
    .await;
    assert_eq!(limit_exceeded.status(), StatusCode::BAD_REQUEST);

    let wallet_after_credit = request(
        &app,
        Method::GET,
        "/v1/me/wallet",
        None,
        Some(&access_token),
        None,
    )
    .await;
    assert_eq!(wallet_after_credit.status(), StatusCode::OK);
    let wallet_after_credit_body = json_body(wallet_after_credit).await;
    assert_eq!(wallet_after_credit_body["balance_minor_units"], "1250");

    let (entry_count, debit_total, credit_total): (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*)::BIGINT,
            COALESCE(SUM(amount_minor) FILTER (WHERE entry_side = 'debit'), 0)::BIGINT,
            COALESCE(SUM(amount_minor) FILTER (WHERE entry_side = 'credit'), 0)::BIGINT
        FROM ledger_entries
        WHERE transaction_id = $1
        "#,
    )
    .bind(transaction_id)
    .fetch_one(&database_pool)
    .await
    .expect("ledger entries should be queryable");
    assert_eq!(entry_count, 2);
    assert_eq!(debit_total, 1250);
    assert_eq!(credit_total, 1250);

    let wallet_balance_columns: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'sandbox_wallets'
          AND column_name ILIKE '%balance%'
        "#,
    )
    .fetch_one(&database_pool)
    .await
    .expect("information schema should be queryable");
    assert_eq!(wallet_balance_columns, 0);

    let immutable_entry_result = sqlx::query(
        "UPDATE ledger_entries SET amount_minor = amount_minor + 1 WHERE transaction_id = $1",
    )
    .bind(transaction_id)
    .execute(&database_pool)
    .await;
    assert!(immutable_entry_result.is_err());

    let immutable_transaction_result = sqlx::query(
        "DELETE FROM ledger_transactions WHERE id = $1",
    )
    .bind(transaction_id)
    .execute(&database_pool)
    .await;
    assert!(immutable_transaction_result.is_err());

    let funding_account_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM ledger_accounts WHERE account_code = 'sandbox_funding:PEN'",
    )
    .fetch_one(&database_pool)
    .await
    .expect("sandbox funding account should exist");
    let unbalanced_transaction_id = Uuid::new_v4();
    let mut unbalanced = database_pool
        .begin()
        .await
        .expect("unbalanced transaction should begin");
    sqlx::query(
        r#"
        INSERT INTO ledger_transactions (
            id,
            transaction_kind,
            currency,
            idempotency_key,
            request_fingerprint,
            resulting_balance_minor,
            posted_at_epoch_seconds
        )
        VALUES ($1, 'test_unbalanced', 'PEN', $2, $3, 0, 1)
        "#,
    )
    .bind(unbalanced_transaction_id)
    .bind(format!("unbalanced-{}", Uuid::new_v4()))
    .bind("x".repeat(43))
    .execute(&mut *unbalanced)
    .await
    .expect("unbalanced transaction row should insert before commit");
    sqlx::query(
        r#"
        INSERT INTO ledger_entries (
            id,
            transaction_id,
            account_id,
            entry_side,
            amount_minor,
            created_at_epoch_seconds
        )
        VALUES ($1, $2, $3, 'debit', 1, 1)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(unbalanced_transaction_id)
    .bind(funding_account_id)
    .execute(&mut *unbalanced)
    .await
    .expect("single entry should insert before deferred validation");
    assert!(unbalanced.commit().await.is_err());

    drop(app);
    let restarted_app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should reconnect to persisted wallet ledger");
    let persisted_wallet = request(
        &restarted_app,
        Method::GET,
        "/v1/me/wallet",
        None,
        Some(&access_token),
        None,
    )
    .await;
    assert_eq!(persisted_wallet.status(), StatusCode::OK);
    let persisted_wallet_body = json_body(persisted_wallet).await;
    assert_eq!(persisted_wallet_body["id"], wallet_id.to_string());
    assert_eq!(persisted_wallet_body["balance_minor_units"], "1250");
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    access_token: Option<&str>,
    idempotency_key: Option<&str>,
) -> Response<Body> {
    let mut builder = Request::builder().method(method).uri(uri);

    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(token) = access_token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(key) = idempotency_key {
        builder = builder.header("Idempotency-Key", key);
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
