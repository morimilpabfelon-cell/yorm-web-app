use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{PayActivityCounterparty, PayActivityItem, PayActivityPage, PayReceiptResponse},
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

        let (entry_count, debit_total, credit_total) = sqlx::query_as::<_, (i64, i64, i64)>(
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
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid_cursor())?;
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
