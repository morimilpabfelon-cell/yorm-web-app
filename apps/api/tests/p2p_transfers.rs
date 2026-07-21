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
async fn p2p_transfer_is_atomic_idempotent_immutable_and_persistent() {
    let database_url = database_url();
    let pool = PgPool::connect(&database_url)
        .await
        .expect("PostgreSQL should accept connections");
    let app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should initialize with PostgreSQL");

    let sender = create_identity_session_wallet(&app, "PE", true).await;
    let recipient = create_identity_session_wallet(&app, "PE", false).await;
    let foreign_recipient = create_identity_session_wallet(&app, "BR", false).await;

    let credit = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "2000" })),
        Some(&sender.access_token),
        Some(&unique_key("credit")),
    )
    .await;
    assert_eq!(credit.status(), StatusCode::CREATED);

    let missing_key = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": recipient.identity_id,
            "amount_minor_units": "750"
        })),
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(missing_key.status(), StatusCode::BAD_REQUEST);

    let transfer_key = unique_key("transfer");
    let transfer = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": recipient.identity_id,
            "amount_minor_units": "750"
        })),
        Some(&sender.access_token),
        Some(&transfer_key),
    )
    .await;
    assert_eq!(transfer.status(), StatusCode::CREATED);
    let transfer_body = json_body(transfer).await;
    let transaction_id = uuid_field(&transfer_body, "transaction_id");
    assert_eq!(transfer_body["transaction_kind"], "sandbox_p2p_transfer");
    assert_eq!(transfer_body["sender_wallet_id"], sender.wallet_id.to_string());
    assert_eq!(
        transfer_body["recipient_wallet_id"],
        recipient.wallet_id.to_string()
    );
    assert_eq!(transfer_body["currency"], "PEN");
    assert_eq!(transfer_body["amount_minor_units"], "750");
    assert_eq!(transfer_body["sender_balance_after_minor_units"], "1250");
    assert_eq!(
        transfer_body["recipient_balance_after_minor_units"],
        "750"
    );

    let replay = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": recipient.identity_id,
            "amount_minor_units": "750"
        })),
        Some(&sender.access_token),
        Some(&transfer_key),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::CREATED);
    let replay_body = json_body(replay).await;
    assert_eq!(replay_body["transaction_id"], transaction_id.to_string());
    assert_eq!(replay_body["sender_balance_after_minor_units"], "1250");
    assert_eq!(
        replay_body["recipient_balance_after_minor_units"],
        "750"
    );

    let conflicting_replay = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": recipient.identity_id,
            "amount_minor_units": "751"
        })),
        Some(&sender.access_token),
        Some(&transfer_key),
    )
    .await;
    assert_eq!(conflicting_replay.status(), StatusCode::CONFLICT);

    let self_transfer = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": sender.identity_id,
            "amount_minor_units": "1"
        })),
        Some(&sender.access_token),
        Some(&unique_key("self")),
    )
    .await;
    assert_eq!(self_transfer.status(), StatusCode::BAD_REQUEST);

    let currency_mismatch = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": foreign_recipient.identity_id,
            "amount_minor_units": "1"
        })),
        Some(&sender.access_token),
        Some(&unique_key("currency")),
    )
    .await;
    assert_eq!(currency_mismatch.status(), StatusCode::CONFLICT);

    let transfer_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM sandbox_p2p_transfers WHERE sender_wallet_id = $1",
    )
    .bind(sender.wallet_id)
    .fetch_one(&pool)
    .await
    .expect("P2P metadata should be queryable");

    let insufficient = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": recipient.identity_id,
            "amount_minor_units": "2000"
        })),
        Some(&sender.access_token),
        Some(&unique_key("insufficient")),
    )
    .await;
    assert_eq!(insufficient.status(), StatusCode::CONFLICT);

    let transfer_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM sandbox_p2p_transfers WHERE sender_wallet_id = $1",
    )
    .bind(sender.wallet_id)
    .fetch_one(&pool)
    .await
    .expect("P2P metadata should remain queryable");
    assert_eq!(transfer_count_after, transfer_count_before);

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
    .fetch_one(&pool)
    .await
    .expect("transfer entries should be queryable");
    assert_eq!(entry_count, 2);
    assert_eq!(debit_total, 750);
    assert_eq!(credit_total, 750);

    let metadata: (Uuid, Uuid, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            sender_wallet_id,
            recipient_wallet_id,
            amount_minor,
            sender_balance_after_minor,
            recipient_balance_after_minor
        FROM sandbox_p2p_transfers
        WHERE transaction_id = $1
        "#,
    )
    .bind(transaction_id)
    .fetch_one(&pool)
    .await
    .expect("P2P metadata should exist");
    assert_eq!(metadata.0, sender.wallet_id);
    assert_eq!(metadata.1, recipient.wallet_id);
    assert_eq!(metadata.2, 750);
    assert_eq!(metadata.3, 1250);
    assert_eq!(metadata.4, 750);

    let immutable_metadata = sqlx::query(
        "UPDATE sandbox_p2p_transfers SET amount_minor = amount_minor + 1 WHERE transaction_id = $1",
    )
    .bind(transaction_id)
    .execute(&pool)
    .await;
    assert!(immutable_metadata.is_err());

    drop(app);
    let restarted = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should reconnect to the persisted ledger");
    assert_wallet_balance(&restarted, &sender.access_token, "1250").await;
    assert_wallet_balance(&restarted, &recipient.access_token, "750").await;
}

#[tokio::test]
async fn concurrent_transfers_cannot_double_spend_sender_balance() {
    let database_url = database_url();
    let pool = PgPool::connect(&database_url)
        .await
        .expect("PostgreSQL should accept connections");
    let app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should initialize with PostgreSQL");

    let sender = create_identity_session_wallet(&app, "PE", true).await;
    let first_recipient = create_identity_session_wallet(&app, "PE", false).await;
    let second_recipient = create_identity_session_wallet(&app, "PE", false).await;

    let credit = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "1000" })),
        Some(&sender.access_token),
        Some(&unique_key("concurrent-credit")),
    )
    .await;
    assert_eq!(credit.status(), StatusCode::CREATED);

    let first_request = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": first_recipient.identity_id,
            "amount_minor_units": "750"
        })),
        Some(&sender.access_token),
        Some(&unique_key("concurrent-one")),
    );
    let second_request = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": second_recipient.identity_id,
            "amount_minor_units": "750"
        })),
        Some(&sender.access_token),
        Some(&unique_key("concurrent-two")),
    );

    let (first_response, second_response) = tokio::join!(first_request, second_request);
    let statuses = [first_response.status(), second_response.status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );

    assert_wallet_balance(&app, &sender.access_token, "250").await;

    let first_balance = wallet_balance_value(&app, &first_recipient.access_token).await;
    let second_balance = wallet_balance_value(&app, &second_recipient.access_token).await;
    assert_eq!(first_balance + second_balance, 750);
    assert!(matches!((first_balance, second_balance), (750, 0) | (0, 750)));

    let transfer_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM sandbox_p2p_transfers WHERE sender_wallet_id = $1",
    )
    .bind(sender.wallet_id)
    .fetch_one(&pool)
    .await
    .expect("sender transfer count should be queryable");
    assert_eq!(transfer_count, 1);

    let sender_debits: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(entry.amount_minor), 0)::BIGINT
        FROM ledger_entries AS entry
        WHERE entry.account_id = (
            SELECT ledger_account_id
            FROM sandbox_wallets
            WHERE id = $1
        )
          AND entry.entry_side = 'debit'
        "#,
    )
    .bind(sender.wallet_id)
    .fetch_one(&pool)
    .await
    .expect("sender debits should be queryable");
    assert_eq!(sender_debits, 750);
}

struct TestActor {
    identity_id: Uuid,
    wallet_id: Uuid,
    access_token: String,
}

async fn create_identity_session_wallet(
    app: &Router,
    country_code: &str,
    configure_pin: bool,
) -> TestActor {
    let identity_response = request(
        app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": format!("p2p-{}@yorm.local", Uuid::new_v4()),
            "display_name": "P2P Test",
            "country_code": country_code
        })),
        None,
        None,
    )
    .await;
    assert_eq!(identity_response.status(), StatusCode::CREATED);
    let identity = json_body(identity_response).await;
    let identity_id = uuid_field(&identity, "id");

    let session_response = request(
        app,
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
        .expect("session response should contain access token")
        .to_owned();

    if configure_pin {
        let pin_response = request(
            app,
            Method::PUT,
            "/v1/me/pin",
            Some(json!({ "pin": "4096" })),
            Some(&access_token),
            None,
        )
        .await;
        assert_eq!(pin_response.status(), StatusCode::NO_CONTENT);
    }

    let wallet_response = request(
        app,
        Method::POST,
        "/v1/me/wallet",
        None,
        Some(&access_token),
        None,
    )
    .await;
    assert_eq!(wallet_response.status(), StatusCode::CREATED);
    let wallet = json_body(wallet_response).await;

    TestActor {
        identity_id,
        wallet_id: uuid_field(&wallet, "id"),
        access_token,
    }
}

async fn assert_wallet_balance(app: &Router, access_token: &str, expected: &str) {
    let wallet = request(
        app,
        Method::GET,
        "/v1/me/wallet",
        None,
        Some(access_token),
        None,
    )
    .await;
    assert_eq!(wallet.status(), StatusCode::OK);
    let body = json_body(wallet).await;
    assert_eq!(body["balance_minor_units"], expected);
}

async fn wallet_balance_value(app: &Router, access_token: &str) -> i64 {
    let wallet = request(
        app,
        Method::GET,
        "/v1/me/wallet",
        None,
        Some(access_token),
        None,
    )
    .await;
    assert_eq!(wallet.status(), StatusCode::OK);
    let body = json_body(wallet).await;
    body["balance_minor_units"]
        .as_str()
        .expect("wallet balance should be a string")
        .parse::<i64>()
        .expect("wallet balance should fit i64")
}

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured for PostgreSQL integration tests")
}

fn unique_key(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}

fn uuid_field(value: &Value, field: &str) -> Uuid {
    Uuid::parse_str(
        value[field]
            .as_str()
            .unwrap_or_else(|| panic!("response should contain {field}")),
    )
    .unwrap_or_else(|_| panic!("{field} should be a UUID"))
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
