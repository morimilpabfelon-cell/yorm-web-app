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
async fn activity_and_receipts_are_authorized_paginated_and_ledger_derived() {
    let database_url = database_url();
    let pool = PgPool::connect(&database_url)
        .await
        .expect("PostgreSQL should accept connections");
    let app = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should initialize with PostgreSQL");

    let sender = create_actor(&app, "Activity Sender", true).await;
    let recipient = create_actor(&app, "Activity Recipient", false).await;
    let outsider = create_actor(&app, "Activity Outsider", false).await;

    let credit_response = request(
        &app,
        Method::POST,
        "/v1/sandbox/wallet/credits",
        Some(json!({ "amount_minor_units": "2000" })),
        Some(&sender.access_token),
        Some(&unique_key("activity-credit")),
    )
    .await;
    assert_eq!(credit_response.status(), StatusCode::CREATED);
    let credit = json_body(credit_response).await;
    let credit_transaction_id = uuid_field(&credit, "transaction_id");

    let transfer_response = request(
        &app,
        Method::POST,
        "/v1/sandbox/transfers",
        Some(json!({
            "recipient_identity_id": recipient.identity_id,
            "amount_minor_units": "750"
        })),
        Some(&sender.access_token),
        Some(&unique_key("activity-transfer")),
    )
    .await;
    assert_eq!(transfer_response.status(), StatusCode::CREATED);
    let transfer = json_body(transfer_response).await;
    let transfer_transaction_id = uuid_field(&transfer, "transaction_id");

    let transaction_count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM ledger_transactions")
            .fetch_one(&pool)
            .await
            .expect("transaction count should be queryable");
    let entry_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM ledger_entries")
        .fetch_one(&pool)
        .await
        .expect("entry count should be queryable");

    let first_page_response = request(
        &app,
        Method::GET,
        "/v1/me/activity?limit=1",
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(first_page_response.status(), StatusCode::OK);
    let first_page = json_body(first_page_response).await;
    assert_eq!(first_page["module"], "Pay Activity");
    assert_eq!(first_page["environment"], "sandbox");
    assert_eq!(first_page["items"].as_array().unwrap().len(), 1);
    let cursor = first_page["next_cursor"]
        .as_str()
        .expect("first page should expose a cursor");

    let second_page_response = request(
        &app,
        Method::GET,
        &format!("/v1/me/activity?limit=1&cursor={cursor}"),
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(second_page_response.status(), StatusCode::OK);
    let second_page = json_body(second_page_response).await;
    assert_eq!(second_page["items"].as_array().unwrap().len(), 1);
    assert!(second_page["next_cursor"].is_null());

    let sender_items = [
        first_page["items"][0].clone(),
        second_page["items"][0].clone(),
    ];
    let sender_credit = find_activity(&sender_items, credit_transaction_id);
    assert_eq!(sender_credit["transaction_kind"], "sandbox_credit");
    assert_eq!(sender_credit["direction"], "credit");
    assert_eq!(sender_credit["amount_minor_units"], "2000");
    assert_eq!(sender_credit["balance_after_minor_units"], "2000");
    assert!(sender_credit["counterparty"].is_null());
    assert_eq!(sender_credit["receipt_available"], true);

    let sender_transfer = find_activity(&sender_items, transfer_transaction_id);
    assert_eq!(sender_transfer["transaction_kind"], "sandbox_p2p_transfer");
    assert_eq!(sender_transfer["direction"], "debit");
    assert_eq!(sender_transfer["amount_minor_units"], "750");
    assert_eq!(sender_transfer["balance_after_minor_units"], "1250");
    assert_eq!(
        sender_transfer["counterparty"]["identity_id"],
        recipient.identity_id.to_string()
    );
    assert_eq!(
        sender_transfer["counterparty"]["display_name"],
        "Activity Recipient"
    );

    let recipient_activity_response = request(
        &app,
        Method::GET,
        "/v1/me/activity",
        None,
        Some(&recipient.access_token),
        None,
    )
    .await;
    assert_eq!(recipient_activity_response.status(), StatusCode::OK);
    let recipient_activity = json_body(recipient_activity_response).await;
    assert_eq!(recipient_activity["items"].as_array().unwrap().len(), 1);
    let recipient_transfer = &recipient_activity["items"][0];
    assert_eq!(recipient_transfer["direction"], "credit");
    assert_eq!(recipient_transfer["balance_after_minor_units"], "750");
    assert_eq!(
        recipient_transfer["counterparty"]["identity_id"],
        sender.identity_id.to_string()
    );
    assert_eq!(
        recipient_transfer["counterparty"]["display_name"],
        "Activity Sender"
    );

    let sender_receipt_response = request(
        &app,
        Method::GET,
        &format!("/v1/me/receipts/{transfer_transaction_id}"),
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(sender_receipt_response.status(), StatusCode::OK);
    let sender_receipt = json_body(sender_receipt_response).await;
    assert_receipt(&sender_receipt, "debit", "1250", "750");
    let sender_reference = sender_receipt["receipt_reference"]
        .as_str()
        .expect("receipt should contain a reference")
        .to_owned();
    assert_eq!(sender_reference.len(), 43);

    let recipient_receipt_response = request(
        &app,
        Method::GET,
        &format!("/v1/me/receipts/{transfer_transaction_id}"),
        None,
        Some(&recipient.access_token),
        None,
    )
    .await;
    assert_eq!(recipient_receipt_response.status(), StatusCode::OK);
    let recipient_receipt = json_body(recipient_receipt_response).await;
    assert_receipt(&recipient_receipt, "credit", "750", "750");
    assert_ne!(
        recipient_receipt["receipt_reference"],
        sender_receipt["receipt_reference"]
    );

    let credit_receipt_response = request(
        &app,
        Method::GET,
        &format!("/v1/me/receipts/{credit_transaction_id}"),
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(credit_receipt_response.status(), StatusCode::OK);
    let credit_receipt = json_body(credit_receipt_response).await;
    assert_eq!(credit_receipt["transaction_kind"], "sandbox_credit");
    assert_eq!(credit_receipt["direction"], "credit");
    assert_eq!(credit_receipt["balance_after_minor_units"], "2000");
    assert_eq!(credit_receipt["ledger_debit_total_minor_units"], "2000");
    assert_eq!(credit_receipt["ledger_credit_total_minor_units"], "2000");

    let outsider_receipt = request(
        &app,
        Method::GET,
        &format!("/v1/me/receipts/{transfer_transaction_id}"),
        None,
        Some(&outsider.access_token),
        None,
    )
    .await;
    assert_eq!(outsider_receipt.status(), StatusCode::NOT_FOUND);

    let malformed_cursor = request(
        &app,
        Method::GET,
        "/v1/me/activity?cursor=not-a-valid-cursor",
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(malformed_cursor.status(), StatusCode::BAD_REQUEST);

    let invalid_limit = request(
        &app,
        Method::GET,
        "/v1/me/activity?limit=101",
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(invalid_limit.status(), StatusCode::BAD_REQUEST);

    let transaction_count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM ledger_transactions")
            .fetch_one(&pool)
            .await
            .expect("transaction count should remain queryable");
    let entry_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM ledger_entries")
        .fetch_one(&pool)
        .await
        .expect("entry count should remain queryable");
    assert_eq!(transaction_count_after, transaction_count_before);
    assert_eq!(entry_count_after, entry_count_before);

    let projection_table_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM information_schema.tables
        WHERE table_schema = 'public'
          AND table_name IN ('pay_activity', 'pay_receipts', 'activity', 'receipts')
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("projection table count should be queryable");
    assert_eq!(projection_table_count, 0);

    drop(app);
    let restarted = yorm_api::app_with_database(&database_url)
        .await
        .expect("application should reconnect to PostgreSQL");
    let persisted_receipt_response = request(
        &restarted,
        Method::GET,
        &format!("/v1/me/receipts/{transfer_transaction_id}"),
        None,
        Some(&sender.access_token),
        None,
    )
    .await;
    assert_eq!(persisted_receipt_response.status(), StatusCode::OK);
    let persisted_receipt = json_body(persisted_receipt_response).await;
    assert_eq!(persisted_receipt["receipt_reference"], sender_reference);
}

fn assert_receipt(receipt: &Value, direction: &str, balance_after: &str, amount: &str) {
    assert_eq!(receipt["module"], "Pay Receipt");
    assert_eq!(receipt["environment"], "sandbox");
    assert_eq!(receipt["receipt_version"], 1);
    assert_eq!(receipt["status"], "posted");
    assert_eq!(receipt["direction"], direction);
    assert_eq!(receipt["currency"], "PEN");
    assert_eq!(receipt["amount_minor_units"], amount);
    assert_eq!(receipt["balance_after_minor_units"], balance_after);
    assert_eq!(receipt["ledger_entry_count"], 2);
    assert_eq!(receipt["ledger_debit_total_minor_units"], amount);
    assert_eq!(receipt["ledger_credit_total_minor_units"], amount);
}

fn find_activity(items: &[Value], transaction_id: Uuid) -> &Value {
    items
        .iter()
        .find(|item| item["transaction_id"] == transaction_id.to_string())
        .expect("activity should contain the transaction")
}

struct TestActor {
    identity_id: Uuid,
    access_token: String,
}

async fn create_actor(app: &Router, display_name: &str, configure_pin: bool) -> TestActor {
    let identity_response = request(
        app,
        Method::POST,
        "/v1/sandbox/identities",
        Some(json!({
            "email": format!("activity-{}@yorm.local", Uuid::new_v4()),
            "display_name": display_name,
            "country_code": "PE"
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

    TestActor {
        identity_id,
        access_token,
    }
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
