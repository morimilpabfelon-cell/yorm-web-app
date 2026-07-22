from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"missing expected block in {path}: {old[:100]!r}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


write(
    "apps/api/src/store/activity.rs",
    r'''use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{
        PayActivityCounterparty, PayActivityItem, PayActivityPage, PayReceiptResponse,
    },
};

use super::{SandboxStore, StoreBackend, digest_token};

const DEFAULT_ACTIVITY_LIMIT: u16 = 20;
const MAX_ACTIVITY_LIMIT: u16 = 100;
const SANDBOX_CREDIT_KIND: &str = "sandbox_credit";
const SANDBOX_P2P_TRANSFER_KIND: &str = "sandbox_p2p_transfer";

#[derive(Clone)]
struct ActivityStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct ActivityRow {
    transaction_id: Uuid,
    transaction_kind: String,
    wallet_id: Uuid,
    currency: String,
    entry_side: String,
    amount_minor: i64,
    balance_after_minor: i64,
    posted_at_epoch_seconds: i64,
    counterparty_identity_id: Option<Uuid>,
    counterparty_display_name: Option<String>,
}

struct ActivityCursor {
    posted_at_epoch_seconds: i64,
    transaction_id: Uuid,
}

impl ActivityStore {
    fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn list_activity(
        &self,
        identity_id: Uuid,
        requested_limit: Option<u16>,
        cursor: Option<&str>,
    ) -> Result<PayActivityPage, ApiError> {
        let limit = validate_limit(requested_limit)?;
        let cursor = decode_cursor(cursor)?;
        let cursor_epoch = cursor.as_ref().map(|value| value.posted_at_epoch_seconds);
        let cursor_transaction_id = cursor.as_ref().map(|value| value.transaction_id);
        let fetch_limit = i64::from(limit) + 1;

        let mut rows = sqlx::query_as::<_, ActivityRow>(
            r#"
            SELECT
                transaction.id AS transaction_id,
                transaction.transaction_kind,
                wallet.id AS wallet_id,
                transaction.currency,
                own_entry.entry_side,
                own_entry.amount_minor,
                CASE
                    WHEN transaction.transaction_kind = 'sandbox_credit'
                        THEN transaction.resulting_balance_minor
                    WHEN p2p.sender_wallet_id = wallet.id
                        THEN p2p.sender_balance_after_minor
                    WHEN p2p.recipient_wallet_id = wallet.id
                        THEN p2p.recipient_balance_after_minor
                    ELSE transaction.resulting_balance_minor
                END AS balance_after_minor,
                transaction.posted_at_epoch_seconds,
                CASE
                    WHEN p2p.sender_wallet_id = wallet.id THEN recipient_identity.id
                    WHEN p2p.recipient_wallet_id = wallet.id THEN sender_identity.id
                    ELSE NULL
                END AS counterparty_identity_id,
                CASE
                    WHEN p2p.sender_wallet_id = wallet.id THEN recipient_identity.display_name
                    WHEN p2p.recipient_wallet_id = wallet.id THEN sender_identity.display_name
                    ELSE NULL
                END AS counterparty_display_name
            FROM sandbox_wallets AS wallet
            INNER JOIN ledger_entries AS own_entry
                ON own_entry.account_id = wallet.ledger_account_id
            INNER JOIN ledger_transactions AS transaction
                ON transaction.id = own_entry.transaction_id
            LEFT JOIN sandbox_p2p_transfers AS p2p
                ON p2p.transaction_id = transaction.id
            LEFT JOIN sandbox_wallets AS sender_wallet
                ON sender_wallet.id = p2p.sender_wallet_id
            LEFT JOIN sandbox_identities AS sender_identity
                ON sender_identity.id = sender_wallet.identity_id
            LEFT JOIN sandbox_wallets AS recipient_wallet
                ON recipient_wallet.id = p2p.recipient_wallet_id
            LEFT JOIN sandbox_identities AS recipient_identity
                ON recipient_identity.id = recipient_wallet.identity_id
            WHERE wallet.identity_id = $1
              AND transaction.transaction_kind IN ('sandbox_credit', 'sandbox_p2p_transfer')
              AND (
                    $2::BIGINT IS NULL
                    OR (transaction.posted_at_epoch_seconds, transaction.id)
                        < ($2::BIGINT, $3::UUID)
              )
            ORDER BY transaction.posted_at_epoch_seconds DESC, transaction.id DESC
            LIMIT $4
            "#,
        )
        .bind(identity_id)
        .bind(cursor_epoch)
        .bind(cursor_transaction_id)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("list Pay Activity", error))?;

        let has_more = rows.len() > usize::from(limit);
        rows.truncate(usize::from(limit));
        let next_cursor = if has_more {
            rows.last().map(encode_cursor).transpose()?
        } else {
            None
        };
        let items = rows
            .into_iter()
            .map(ActivityRow::into_item)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PayActivityPage {
            module: "Pay Activity",
            environment: "sandbox",
            items,
            next_cursor,
        })
    }

    async fn get_receipt(
        &self,
        identity_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<PayReceiptResponse, ApiError> {
        let row = load_activity_row(&self.pool, identity_id, transaction_id)
            .await?
            .ok_or_else(|| {
                ApiError::not_found(
                    "RECEIPT_NOT_FOUND",
                    "posted transaction was not found for the authenticated wallet",
                )
            })?;

        let (entry_count, debit_total, credit_total) =
            sqlx::query_as::<_, (i64, i64, i64)>(
                r#"
                SELECT
                    COUNT(*)::BIGINT,
                    COALESCE(
                        SUM(amount_minor) FILTER (WHERE entry_side = 'debit'),
                        0
                    )::BIGINT,
                    COALESCE(
                        SUM(amount_minor) FILTER (WHERE entry_side = 'credit'),
                        0
                    )::BIGINT
                FROM ledger_entries
                WHERE transaction_id = $1
                "#,
            )
            .bind(transaction_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| database_error("calculate Pay Receipt ledger totals", error))?;

        if entry_count < 2 || debit_total != credit_total || debit_total <= 0 {
            return Err(ApiError::internal(
                "posted ledger transaction is not balanced; Pay Receipt was not generated",
            ));
        }

        let item = row.into_item()?;
        let counterparty_reference = item
            .counterparty
            .as_ref()
            .map(|counterparty| counterparty.identity_id.to_string())
            .unwrap_or_else(|| "sandbox_funding".to_owned());
        let canonical = format!(
            "pay_receipt_v1|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            item.transaction_id,
            item.transaction_kind,
            item.wallet_id,
            item.direction,
            item.currency,
            item.amount_minor_units,
            item.balance_after_minor_units,
            item.posted_at_epoch_seconds,
            entry_count,
            debit_total,
            credit_total,
            counterparty_reference,
        );

        Ok(PayReceiptResponse {
            module: "Pay Receipt",
            environment: "sandbox",
            receipt_version: 1,
            receipt_reference: digest_token(&canonical),
            transaction_id: item.transaction_id,
            transaction_kind: item.transaction_kind,
            status: "posted",
            direction: item.direction,
            wallet_id: item.wallet_id,
            counterparty: item.counterparty,
            currency: item.currency,
            amount_minor_units: item.amount_minor_units,
            balance_after_minor_units: item.balance_after_minor_units,
            posted_at_epoch_seconds: item.posted_at_epoch_seconds,
            ledger_entry_count: entry_count,
            ledger_debit_total_minor_units: debit_total.to_string(),
            ledger_credit_total_minor_units: credit_total.to_string(),
        })
    }
}

impl SandboxStore {
    pub async fn list_activity(
        &self,
        identity_id: Uuid,
        requested_limit: Option<u16>,
        cursor: Option<&str>,
    ) -> Result<PayActivityPage, ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Err(ApiError::service_unavailable(
                "DATABASE_REQUIRED",
                "Pay Activity requires the PostgreSQL sandbox backend",
            )),
            StoreBackend::Postgres { identity, .. } => {
                ActivityStore::new(identity.pool())
                    .list_activity(identity_id, requested_limit, cursor)
                    .await
            }
        }
    }

    pub async fn get_receipt(
        &self,
        identity_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<PayReceiptResponse, ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Err(ApiError::service_unavailable(
                "DATABASE_REQUIRED",
                "Pay Receipt requires the PostgreSQL sandbox backend",
            )),
            StoreBackend::Postgres { identity, .. } => {
                ActivityStore::new(identity.pool())
                    .get_receipt(identity_id, transaction_id)
                    .await
            }
        }
    }
}

impl ActivityRow {
    fn into_item(self) -> Result<PayActivityItem, ApiError> {
        let counterparty = match (
            self.counterparty_identity_id,
            self.counterparty_display_name,
        ) {
            (Some(identity_id), Some(display_name)) => Some(PayActivityCounterparty {
                identity_id,
                display_name,
            }),
            (None, None) => None,
            _ => {
                return Err(ApiError::internal(
                    "Pay Activity counterparty projection is incomplete",
                ));
            }
        };

        Ok(PayActivityItem {
            transaction_id: self.transaction_id,
            transaction_kind: self.transaction_kind,
            wallet_id: self.wallet_id,
            direction: self.entry_side,
            currency: self.currency,
            amount_minor_units: self.amount_minor.to_string(),
            balance_after_minor_units: self.balance_after_minor.to_string(),
            counterparty,
            posted_at_epoch_seconds: from_database_epoch(self.posted_at_epoch_seconds)?,
            receipt_available: true,
        })
    }
}

async fn load_activity_row(
    pool: &PgPool,
    identity_id: Uuid,
    transaction_id: Uuid,
) -> Result<Option<ActivityRow>, ApiError> {
    sqlx::query_as::<_, ActivityRow>(
        r#"
        SELECT
            transaction.id AS transaction_id,
            transaction.transaction_kind,
            wallet.id AS wallet_id,
            transaction.currency,
            own_entry.entry_side,
            own_entry.amount_minor,
            CASE
                WHEN transaction.transaction_kind = 'sandbox_credit'
                    THEN transaction.resulting_balance_minor
                WHEN p2p.sender_wallet_id = wallet.id
                    THEN p2p.sender_balance_after_minor
                WHEN p2p.recipient_wallet_id = wallet.id
                    THEN p2p.recipient_balance_after_minor
                ELSE transaction.resulting_balance_minor
            END AS balance_after_minor,
            transaction.posted_at_epoch_seconds,
            CASE
                WHEN p2p.sender_wallet_id = wallet.id THEN recipient_identity.id
                WHEN p2p.recipient_wallet_id = wallet.id THEN sender_identity.id
                ELSE NULL
            END AS counterparty_identity_id,
            CASE
                WHEN p2p.sender_wallet_id = wallet.id THEN recipient_identity.display_name
                WHEN p2p.recipient_wallet_id = wallet.id THEN sender_identity.display_name
                ELSE NULL
            END AS counterparty_display_name
        FROM sandbox_wallets AS wallet
        INNER JOIN ledger_entries AS own_entry
            ON own_entry.account_id = wallet.ledger_account_id
        INNER JOIN ledger_transactions AS transaction
            ON transaction.id = own_entry.transaction_id
        LEFT JOIN sandbox_p2p_transfers AS p2p
            ON p2p.transaction_id = transaction.id
        LEFT JOIN sandbox_wallets AS sender_wallet
            ON sender_wallet.id = p2p.sender_wallet_id
        LEFT JOIN sandbox_identities AS sender_identity
            ON sender_identity.id = sender_wallet.identity_id
        LEFT JOIN sandbox_wallets AS recipient_wallet
            ON recipient_wallet.id = p2p.recipient_wallet_id
        LEFT JOIN sandbox_identities AS recipient_identity
            ON recipient_identity.id = recipient_wallet.identity_id
        WHERE wallet.identity_id = $1
          AND transaction.id = $2
          AND transaction.transaction_kind IN ('sandbox_credit', 'sandbox_p2p_transfer')
        "#,
    )
    .bind(identity_id)
    .bind(transaction_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| database_error("load Pay Receipt transaction", error))
}

fn validate_limit(requested_limit: Option<u16>) -> Result<u16, ApiError> {
    let limit = requested_limit.unwrap_or(DEFAULT_ACTIVITY_LIMIT);
    if !(1..=MAX_ACTIVITY_LIMIT).contains(&limit) {
        return Err(ApiError::bad_request(
            "ACTIVITY_LIMIT_INVALID",
            format!("activity limit must be between 1 and {MAX_ACTIVITY_LIMIT}"),
        ));
    }
    Ok(limit)
}

fn encode_cursor(row: &ActivityRow) -> Result<String, ApiError> {
    if row.posted_at_epoch_seconds < 0 {
        return Err(ApiError::internal(
            "Pay Activity contains a negative posting timestamp",
        ));
    }
    Ok(URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        row.posted_at_epoch_seconds, row.transaction_id
    )))
}

fn decode_cursor(cursor: Option<&str>) -> Result<Option<ActivityCursor>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let decoded = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| invalid_cursor())?;
    let decoded = std::str::from_utf8(&decoded).map_err(|_| invalid_cursor())?;
    let (epoch, transaction_id) = decoded.split_once(':').ok_or_else(invalid_cursor)?;
    let posted_at_epoch_seconds = epoch.parse::<i64>().map_err(|_| invalid_cursor())?;
    if posted_at_epoch_seconds < 0 {
        return Err(invalid_cursor());
    }
    let transaction_id = Uuid::parse_str(transaction_id).map_err(|_| invalid_cursor())?;
    Ok(Some(ActivityCursor {
        posted_at_epoch_seconds,
        transaction_id,
    }))
}

fn invalid_cursor() -> ApiError {
    ApiError::bad_request(
        "ACTIVITY_CURSOR_INVALID",
        "activity cursor is invalid or malformed",
    )
}

fn from_database_epoch(value: i64) -> Result<u64, ApiError> {
    u64::try_from(value).map_err(|_| ApiError::internal("database timestamp is negative"))
}

fn database_error(context: &str, error: sqlx::Error) -> ApiError {
    tracing::error!(context, database_error = %error, "PostgreSQL operation failed");
    ApiError::internal(format!("{context} failed"))
}
''',
)

replace_once(
    "apps/api/src/store.rs",
    "mod ledger;\nmod postgres;",
    "mod activity;\nmod ledger;\nmod postgres;",
)

replace_once(
    "apps/api/src/model.rs",
    "pub struct SandboxTransferResponse {\n    pub transaction_id: Uuid,\n    pub transaction_kind: String,\n    pub sender_wallet_id: Uuid,\n    pub recipient_wallet_id: Uuid,\n    pub currency: String,\n    pub amount_minor_units: String,\n    pub sender_balance_after_minor_units: String,\n    pub recipient_balance_after_minor_units: String,\n    pub posted_at_epoch_seconds: u64,\n}\n",
    "pub struct SandboxTransferResponse {\n    pub transaction_id: Uuid,\n    pub transaction_kind: String,\n    pub sender_wallet_id: Uuid,\n    pub recipient_wallet_id: Uuid,\n    pub currency: String,\n    pub amount_minor_units: String,\n    pub sender_balance_after_minor_units: String,\n    pub recipient_balance_after_minor_units: String,\n    pub posted_at_epoch_seconds: u64,\n}\n\n#[derive(Debug, Deserialize)]\npub struct PayActivityQuery {\n    pub limit: Option<u16>,\n    pub cursor: Option<String>,\n}\n\n#[derive(Debug, Clone, Serialize)]\npub struct PayActivityCounterparty {\n    pub identity_id: Uuid,\n    pub display_name: String,\n}\n\n#[derive(Debug, Clone, Serialize)]\npub struct PayActivityItem {\n    pub transaction_id: Uuid,\n    pub transaction_kind: String,\n    pub wallet_id: Uuid,\n    pub direction: String,\n    pub currency: String,\n    pub amount_minor_units: String,\n    pub balance_after_minor_units: String,\n    pub counterparty: Option<PayActivityCounterparty>,\n    pub posted_at_epoch_seconds: u64,\n    pub receipt_available: bool,\n}\n\n#[derive(Debug, Serialize)]\npub struct PayActivityPage {\n    pub module: &'static str,\n    pub environment: &'static str,\n    pub items: Vec<PayActivityItem>,\n    pub next_cursor: Option<String>,\n}\n\n#[derive(Debug, Serialize)]\npub struct PayReceiptResponse {\n    pub module: &'static str,\n    pub environment: &'static str,\n    pub receipt_version: u8,\n    pub receipt_reference: String,\n    pub transaction_id: Uuid,\n    pub transaction_kind: String,\n    pub status: &'static str,\n    pub direction: String,\n    pub wallet_id: Uuid,\n    pub counterparty: Option<PayActivityCounterparty>,\n    pub currency: String,\n    pub amount_minor_units: String,\n    pub balance_after_minor_units: String,\n    pub posted_at_epoch_seconds: u64,\n    pub ledger_entry_count: i64,\n    pub ledger_debit_total_minor_units: String,\n    pub ledger_credit_total_minor_units: String,\n}\n",
)

replace_once(
    "apps/api/src/lib.rs",
    "extract::State,",
    "extract::{Path, Query, State},",
)
replace_once(
    "apps/api/src/lib.rs",
    "use serde::Serialize;\nuse tower_http::trace::TraceLayer;",
    "use serde::Serialize;\nuse tower_http::trace::TraceLayer;\nuse uuid::Uuid;",
)
replace_once(
    "apps/api/src/lib.rs",
    "CreateIdentityRequest, CreateSessionRequest, IdentityView, PayLimitsResponse, PinRequest,\n        PinVerificationResponse, SandboxCreditRequest, SandboxCreditResponse,\n        SandboxTransferRequest, SandboxTransferResponse, SessionResponse, WalletView,",
    "CreateIdentityRequest, CreateSessionRequest, IdentityView, PayActivityPage, PayActivityQuery,\n        PayLimitsResponse, PayReceiptResponse, PinRequest, PinVerificationResponse,\n        SandboxCreditRequest, SandboxCreditResponse, SandboxTransferRequest,\n        SandboxTransferResponse, SessionResponse, WalletView,",
)
replace_once(
    "apps/api/src/lib.rs",
    ".route(\"/v1/sandbox/transfers\", post(transfer_wallet))\n        .route(\"/v1/me/session\", delete(delete_session))",
    ".route(\"/v1/sandbox/transfers\", post(transfer_wallet))\n        .route(\"/v1/me/activity\", get(get_activity))\n        .route(\"/v1/me/receipts/{transaction_id}\", get(get_receipt))\n        .route(\"/v1/me/session\", delete(delete_session))",
)
replace_once(
    "apps/api/src/lib.rs",
    "async fn delete_session(\n",
    "async fn get_activity(\n    State(state): State<AppState>,\n    headers: HeaderMap,\n    Query(query): Query<PayActivityQuery>,\n) -> Result<Json<PayActivityPage>, ApiError> {\n    let identity = authenticate(&headers, &state).await?;\n    Ok(Json(\n        state\n            .store\n            .list_activity(identity.id, query.limit, query.cursor.as_deref())\n            .await?,\n    ))\n}\n\nasync fn get_receipt(\n    State(state): State<AppState>,\n    headers: HeaderMap,\n    Path(transaction_id): Path<Uuid>,\n) -> Result<Json<PayReceiptResponse>, ApiError> {\n    let identity = authenticate(&headers, &state).await?;\n    Ok(Json(\n        state.store.get_receipt(identity.id, transaction_id).await?,\n    ))\n}\n\nasync fn delete_session(\n",
)

replace_once(
    "packages/contracts/src/index.ts",
    "export interface ApiErrorResponse {",
    "export interface PayActivityCounterparty {\n  readonly identity_id: string;\n  readonly display_name: string;\n}\n\nexport interface PayActivityItem {\n  readonly transaction_id: string;\n  readonly transaction_kind: 'sandbox_credit' | 'sandbox_p2p_transfer';\n  readonly wallet_id: string;\n  readonly direction: 'credit' | 'debit';\n  readonly currency: string;\n  readonly amount_minor_units: string;\n  readonly balance_after_minor_units: string;\n  readonly counterparty: PayActivityCounterparty | null;\n  readonly posted_at_epoch_seconds: number;\n  readonly receipt_available: true;\n}\n\nexport interface PayActivityPage {\n  readonly module: 'Pay Activity';\n  readonly environment: 'sandbox';\n  readonly items: readonly PayActivityItem[];\n  readonly next_cursor: string | null;\n}\n\nexport interface PayReceiptResponse {\n  readonly module: 'Pay Receipt';\n  readonly environment: 'sandbox';\n  readonly receipt_version: 1;\n  readonly receipt_reference: string;\n  readonly transaction_id: string;\n  readonly transaction_kind: 'sandbox_credit' | 'sandbox_p2p_transfer';\n  readonly status: 'posted';\n  readonly direction: 'credit' | 'debit';\n  readonly wallet_id: string;\n  readonly counterparty: PayActivityCounterparty | null;\n  readonly currency: string;\n  readonly amount_minor_units: string;\n  readonly balance_after_minor_units: string;\n  readonly posted_at_epoch_seconds: number;\n  readonly ledger_entry_count: number;\n  readonly ledger_debit_total_minor_units: string;\n  readonly ledger_credit_total_minor_units: string;\n}\n\nexport interface ApiErrorResponse {",
)

write(
    "apps/api/tests/activity_receipts.rs",
    r'''use axum::{
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
    let entry_count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM ledger_entries")
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
    let entry_count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM ledger_entries")
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
''',
)

write(
    "scripts/test-activity-receipts-sandbox.ps1",
    r'''$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$composeFile = Join-Path $repoRoot "infra\docker\compose.yml"
$databaseUrl = "postgres://yorm:yorm_local_only@127.0.0.1:5432/yorm_pay?sslmode=disable"
$apiAddress = "127.0.0.1:8787"
$apiBase = "http://$apiAddress"
$apiExecutable = Join-Path $repoRoot "target\debug\yorm-api.exe"
$apiProcess = $null

function Start-YormApi {
    if (-not (Test-Path $apiExecutable)) {
        throw "Yorm API executable not found: $apiExecutable"
    }
    $env:DATABASE_URL = $databaseUrl
    $env:YORM_API_ADDR = $apiAddress
    $script:apiProcess = Start-Process -FilePath $apiExecutable -WorkingDirectory $repoRoot -PassThru -WindowStyle Hidden
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if ($script:apiProcess.HasExited) { throw "Yorm API exited before becoming ready." }
        try {
            $health = Invoke-RestMethod -Method Get -Uri "$apiBase/health/database" -TimeoutSec 2
            if ($health.status -eq "ok" -and $health.backend -eq "postgres") { return }
        }
        catch { Start-Sleep -Milliseconds 250 }
    }
    throw "Yorm API did not become ready at $apiBase."
}

function Stop-YormApi {
    if ($null -ne $script:apiProcess -and -not $script:apiProcess.HasExited) {
        Stop-Process -Id $script:apiProcess.Id -Force
        $script:apiProcess.WaitForExit()
    }
    $script:apiProcess = $null
}

function Invoke-Json {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Path,
        [string]$Token,
        [object]$Body,
        [string]$IdempotencyKey
    )
    $headers = @{}
    if (-not [string]::IsNullOrWhiteSpace($Token)) { $headers["Authorization"] = "Bearer $Token" }
    if (-not [string]::IsNullOrWhiteSpace($IdempotencyKey)) { $headers["Idempotency-Key"] = $IdempotencyKey }
    $parameters = @{ Method = $Method; Uri = "$apiBase$Path"; Headers = $headers; TimeoutSec = 10 }
    if ($null -ne $Body) {
        $parameters["ContentType"] = "application/json"
        $parameters["Body"] = ($Body | ConvertTo-Json -Compress)
    }
    Invoke-RestMethod @parameters
}

function New-Actor {
    param([Parameter(Mandatory = $true)][string]$Name, [bool]$ConfigurePin)
    $suffix = [Guid]::NewGuid().ToString("N")
    $identity = Invoke-Json -Method Post -Path "/v1/sandbox/identities" -Body @{
        email = "activity-$suffix@yorm.local"
        display_name = $Name
        country_code = "PE"
    }
    $session = Invoke-Json -Method Post -Path "/v1/sandbox/sessions" -Body @{ identity_id = $identity.id }
    $token = [string]$session.access_token
    if ($ConfigurePin) {
        Invoke-Json -Method Put -Path "/v1/me/pin" -Token $token -Body @{ pin = "4096" } | Out-Null
    }
    $wallet = Invoke-Json -Method Post -Path "/v1/me/wallet" -Token $token
    [PSCustomObject]@{ identity = $identity; token = $token; wallet = $wallet }
}

try {
    docker compose -f $composeFile up -d postgres | Out-Host
    docker compose -f $composeFile exec -T postgres pg_isready -U yorm -d yorm_pay | Out-Host
    $env:DATABASE_URL = $databaseUrl
    $env:YORM_API_ADDR = $apiAddress
    cargo build --workspace | Out-Host
    Start-YormApi

    $sender = New-Actor -Name "Activity Sender" -ConfigurePin $true
    $recipient = New-Actor -Name "Activity Recipient" -ConfigurePin $false
    $outsider = New-Actor -Name "Activity Outsider" -ConfigurePin $false
    $suffix = [Guid]::NewGuid().ToString("N")

    $credit = Invoke-Json -Method Post -Path "/v1/sandbox/wallet/credits" -Token $sender.token `
        -IdempotencyKey "activity-credit-$suffix" -Body @{ amount_minor_units = "2000" }
    $transfer = Invoke-Json -Method Post -Path "/v1/sandbox/transfers" -Token $sender.token `
        -IdempotencyKey "activity-transfer-$suffix" -Body @{
            recipient_identity_id = $recipient.identity.id
            amount_minor_units = "750"
        }

    $pageOne = Invoke-Json -Method Get -Path "/v1/me/activity?limit=1" -Token $sender.token
    $pageTwo = Invoke-Json -Method Get -Path "/v1/me/activity?limit=1&cursor=$($pageOne.next_cursor)" -Token $sender.token
    $senderItems = @($pageOne.items) + @($pageTwo.items)
    $senderTransfer = $senderItems | Where-Object { $_.transaction_id -eq $transfer.transaction_id }
    $senderCredit = $senderItems | Where-Object { $_.transaction_id -eq $credit.transaction_id }

    $recipientActivity = Invoke-Json -Method Get -Path "/v1/me/activity" -Token $recipient.token
    $senderReceipt = Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $sender.token
    $recipientReceipt = Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $recipient.token

    $outsiderBlocked = $false
    try {
        Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $outsider.token | Out-Null
    }
    catch {
        if ($_.Exception.Response.StatusCode.value__ -eq 404) { $outsiderBlocked = $true } else { throw }
    }

    $transactionCountBefore = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_transactions;"
    $entryCountBefore = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_entries;"
    $projectionTableCount = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='public' AND table_name IN ('pay_activity','pay_receipts','activity','receipts');"

    Stop-YormApi
    Start-YormApi

    $persistedReceipt = Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $sender.token
    $persistedActivity = Invoke-Json -Method Get -Path "/v1/me/activity" -Token $sender.token
    $transactionCountAfter = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_transactions;"
    $entryCountAfter = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_entries;"

    [PSCustomObject]@{
        database_backend = "postgres"
        activity_module = [string]$pageOne.module
        receipt_module = [string]$senderReceipt.module
        activity_page_one_count = @($pageOne.items).Count
        activity_page_two_count = @($pageTwo.items).Count
        stable_cursor_present = (-not [string]::IsNullOrWhiteSpace([string]$pageOne.next_cursor))
        sender_credit_direction = [string]$senderCredit.direction
        sender_transfer_direction = [string]$senderTransfer.direction
        recipient_transfer_direction = [string]$recipientActivity.items[0].direction
        sender_balance_after_minor_units = [string]$senderTransfer.balance_after_minor_units
        recipient_balance_after_minor_units = [string]$recipientActivity.items[0].balance_after_minor_units
        counterparty_visible = ([string]$senderTransfer.counterparty.identity_id -eq [string]$recipient.identity.id)
        outsider_receipt_blocked = $outsiderBlocked
        receipt_reference_length = ([string]$senderReceipt.receipt_reference).Length
        receipt_perspective_distinct = ([string]$senderReceipt.receipt_reference -ne [string]$recipientReceipt.receipt_reference)
        ledger_entry_count = [int64]$senderReceipt.ledger_entry_count
        ledger_debit_total = [int64]$senderReceipt.ledger_debit_total_minor_units
        ledger_credit_total = [int64]$senderReceipt.ledger_credit_total_minor_units
        receipt_persisted = ([string]$persistedReceipt.receipt_reference -eq [string]$senderReceipt.receipt_reference)
        activity_persisted_count = @($persistedActivity.items).Count
        transaction_count_unchanged = ([int64]$transactionCountBefore.Trim() -eq [int64]$transactionCountAfter.Trim())
        entry_count_unchanged = ([int64]$entryCountBefore.Trim() -eq [int64]$entryCountAfter.Trim())
        projection_table_count = [int64]$projectionTableCount.Trim()
        real_money_enabled = $false
        external_providers_enabled = $false
    } | Format-List
}
finally {
    Stop-YormApi
}
''',
)

write(
    "README.md",
    r'''# Yorm Pay

Repositorio oficial para construir desde cero el software real de **Yorm Pay**.

## Estado

```text
FOUNDATION 2C — IN PROGRESS
SANDBOX ONLY
REAL MONEY DISABLED
```

La fuente de verdad visual y funcional es el diseño original del fundador en Figma. El repositorio anterior no se copia; solo puede consultarse como referencia técnica.

## Arquitectura actual

```text
apps/
  api/       API sandbox Rust/Axum + SQLx/PostgreSQL
  mobile/    frontera futura React Native/Expo
  web/       frontera futura Next.js
  admin/     frontera futura de operaciones
  worker/    frontera futura de tareas y conciliación
packages/
  contracts/      contratos TypeScript
  design-tokens/  paleta y tokens visuales
infra/
  docker/    PostgreSQL local
```

## Requisitos

- Node.js 24
- pnpm 10.34.5
- Rust stable
- Docker Desktop con Docker Compose

## Preparar PostgreSQL local

```powershell
cd C:\Users\morim\yorm-web-app

docker compose -f .\infra\docker\compose.yml up -d postgres

$env:DATABASE_URL = "postgres://yorm:yorm_local_only@127.0.0.1:5432/yorm_pay?sslmode=disable"
$env:YORM_API_ADDR = "127.0.0.1:8787"
```

Las migraciones de `apps/api/migrations` se aplican automáticamente al iniciar la API.

## Validación

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm typecheck
pnpm build
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace
cargo run -p yorm-api
```

API local:

```text
GET    http://127.0.0.1:8787/health
GET    http://127.0.0.1:8787/health/database
GET    http://127.0.0.1:8787/v1/system/status
POST   http://127.0.0.1:8787/v1/me/wallet
GET    http://127.0.0.1:8787/v1/me/wallet
POST   http://127.0.0.1:8787/v1/sandbox/wallet/credits
POST   http://127.0.0.1:8787/v1/sandbox/transfers
GET    http://127.0.0.1:8787/v1/me/activity
GET    http://127.0.0.1:8787/v1/me/receipts/{transaction_id}
DELETE http://127.0.0.1:8787/v1/me/session
```

Validación integral de Foundation 2C en Windows:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\test-activity-receipts-sandbox.ps1
```

## Persistencia sandbox

```text
sandbox_identities
sandbox_sessions
PIN Argon2
contador y bloqueo de PIN
digest SHA-256 de sesión
sandbox_wallets
ledger_accounts
ledger_transactions
ledger_entries
sandbox_p2p_transfers
```

Pay Activity y Pay Receipt no crean tablas adicionales: se derivan del ledger confirmado.

## Invariantes financieras

- Todos los montos usan unidades menores enteras; nunca `float`.
- Los saldos se derivan de asientos y no tienen columna mutable.
- Cada transacción confirmada mantiene débitos iguales a créditos.
- Los asientos, transacciones y metadatos P2P son inmutables.
- Los créditos y transferencias exigen `Idempotency-Key`.
- Las transferencias bloquean las dos wallets en orden determinista.
- Saldo insuficiente no crea transacciones ni asientos parciales.
- Autoenvíos y transferencias entre monedas distintas se rechazan.
- Pay Activity y Pay Receipt son proyecciones de solo lectura.
- Una identidad solo puede consultar operaciones de su propia wallet.
- Los recibos se generan únicamente para transacciones posteadas y balanceadas.

## Seguridad

- Sin dinero real.
- Sin proveedores externos activos.
- Sin KYC/AML en vivo.
- Sin bancos, depósitos o retiros externos.
- Sin pagos a comercios.
- Sin tarjetas ni conversión de divisas.
- Sin claves idempotentes, fingerprints internos ni códigos de cuenta en Activity o Receipt.
- Sin tokens Bearer ni PIN en texto plano dentro de PostgreSQL.
- Sin afirmaciones de producción.

Tracks #11.
''',
)

write(
    "AGENTS.md",
    r'''# Yorm Pay — instrucciones para agentes

## Fuente de verdad

1. Diseño original del fundador en Figma.
2. Issue activo y criterios de aceptación.
3. Este archivo y documentación versionada.

## Reglas obligatorias

- Construir desde cero; no copiar el repositorio anterior.
- Una fase y un pull request estrecho por vez.
- No activar dinero real, proveedores externos ni producción.
- No modificar wallet, ledger, saldos, idempotencia, settlement o reconciliación sin issue R3 separado.
- No presentar datos simulados como reales.
- No generar comprobantes de éxito antes de una confirmación backend verificable.
- Todo monto debe representarse en unidades menores enteras y con moneda explícita.
- Toda operación financiera debe ser atómica, idempotente y auditable.
- Cambios mobile nativos requieren issue y revisión separada.
- La ejecución normal usa PostgreSQL; el backend en memoria existe solo para pruebas unitarias rápidas.
- No registrar PIN, tokens Bearer, hashes Argon2, digests de sesión, claves idempotentes ni `DATABASE_URL` en logs.
- Wallet, ledger y P2P solo operan en sandbox.
- Transacciones, metadatos P2P y asientos posteados son inmutables; todo saldo se deriva del ledger.
- El emisor de una transferencia se deriva exclusivamente de la sesión autenticada.
- Las wallets participantes se bloquean en orden determinista antes de consultar o gastar saldo.
- Una transferencia no puede dejar saldo negativo ni escrituras parciales.
- Transferencias entre monedas distintas, autoenvíos, comercios, bancos, tarjetas y conversión permanecen deshabilitados.
- Pay Activity y Pay Receipt son proyecciones de solo lectura; no pueden crear ni modificar movimientos.
- Una identidad solo puede consultar actividad y recibos de su propia wallet.
- La paginación de actividad debe ser estable por timestamp e identificador de transacción.
- Un recibo solo puede emitirse para una transacción posteada, visible y balanceada.
- No exponer claves idempotentes, fingerprints internos ni códigos de cuenta en respuestas de actividad o recibos.

## Nomenclatura de producto

```text
Yorm Pay
Compliance Layer
Pay Limits
Pay Convert
Pay Exchange Link
Pay QR
Pay Code
Pay Link
Pay Merchant
Pay Touch
Pay Card
Pay Disposable Card
Pay Checkout
Pay Payouts
Pay Gateway
Pay Receipt
Pay Activity
Pay Guide
Pay Safe
Pay Card Liquidity
```

`IonExchange` es un nombre propio externo. No se usa `Ion` como prefijo de módulos de Yorm Pay.

## Paleta oficial

```text
Paper  #F6F4F1
Stone  #E4DED2
Coral  #F95C4B
Black  #000000
```

## Gate actual

```text
Issue #11
Foundation 2C
Pay Activity + Pay Receipt derivados del ledger
Riesgo R3.3
Sandbox only
Sin dinero real, bancos, comercios, tarjetas ni conversión
```
''',
)
