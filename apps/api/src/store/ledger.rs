use sqlx::{PgPool, Postgres, Transaction};
use tracing::error;
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{SandboxCreditResponse, SandboxTransferResponse, WalletView},
};

use super::{SandboxStore, StoreBackend, digest_token};

const SANDBOX_CREDIT_KIND: &str = "sandbox_credit";
const SANDBOX_P2P_TRANSFER_KIND: &str = "sandbox_p2p_transfer";

#[derive(Clone)]
pub(super) struct LedgerStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct WalletRow {
    id: Uuid,
    identity_id: Uuid,
    ledger_account_id: Uuid,
    currency: String,
    created_at_epoch_seconds: i64,
    balance_minor: i64,
}

#[derive(Clone, sqlx::FromRow)]
struct LockedWalletRow {
    id: Uuid,
    identity_id: Uuid,
    ledger_account_id: Uuid,
    currency: String,
    country_code: String,
}

#[derive(sqlx::FromRow)]
struct CreditReplayRow {
    transaction_id: Uuid,
    wallet_id: Uuid,
    transaction_kind: String,
    currency: String,
    amount_minor: i64,
    resulting_balance_minor: i64,
    posted_at_epoch_seconds: i64,
    request_fingerprint: String,
}

#[derive(sqlx::FromRow)]
struct TransferReplayRow {
    transaction_id: Uuid,
    transaction_kind: String,
    sender_wallet_id: Uuid,
    recipient_wallet_id: Uuid,
    currency: String,
    amount_minor: i64,
    sender_balance_after_minor: i64,
    recipient_balance_after_minor: i64,
    posted_at_epoch_seconds: i64,
    request_fingerprint: String,
}

impl LedgerStore {
    pub(super) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(super) async fn create_wallet(
        &self,
        identity_id: Uuid,
        now: u64,
    ) -> Result<WalletView, ApiError> {
        let country_code: String =
            sqlx::query_scalar("SELECT country_code FROM sandbox_identities WHERE id = $1")
                .bind(identity_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| database_error("load identity for wallet", error))?
                .ok_or_else(|| {
                    ApiError::not_found("IDENTITY_NOT_FOUND", "identity does not exist")
                })?;

        let currency = home_currency(&country_code);
        let now_database = to_database_epoch(now)?;
        let wallet_account_code = format!("wallet:{identity_id}:{currency}");
        let funding_account_code = format!("sandbox_funding:{currency}");
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| database_error("begin wallet creation", error))?;

        ensure_account(
            &mut transaction,
            &funding_account_code,
            "asset",
            "debit",
            currency,
            now_database,
        )
        .await?;
        ensure_account(
            &mut transaction,
            &wallet_account_code,
            "liability",
            "credit",
            currency,
            now_database,
        )
        .await?;

        let wallet_account_id: Uuid =
            sqlx::query_scalar("SELECT id FROM ledger_accounts WHERE account_code = $1")
                .bind(&wallet_account_code)
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| database_error("load wallet ledger account", error))?;

        sqlx::query(
            r#"
            INSERT INTO sandbox_wallets (
                id,
                identity_id,
                ledger_account_id,
                currency,
                created_at_epoch_seconds
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (identity_id, currency) DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(identity_id)
        .bind(wallet_account_id)
        .bind(currency)
        .bind(now_database)
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("create sandbox wallet", error))?;

        transaction
            .commit()
            .await
            .map_err(|error| database_error("commit wallet creation", error))?;

        self.get_wallet(identity_id).await
    }

    pub(super) async fn get_wallet(&self, identity_id: Uuid) -> Result<WalletView, ApiError> {
        let row = sqlx::query_as::<_, WalletRow>(
            r#"
            SELECT
                wallet.id,
                wallet.identity_id,
                wallet.ledger_account_id,
                wallet.currency,
                wallet.created_at_epoch_seconds,
                COALESCE(
                    SUM(
                        CASE entry.entry_side
                            WHEN 'credit' THEN entry.amount_minor
                            WHEN 'debit' THEN -entry.amount_minor
                        END
                    ),
                    0
                )::BIGINT AS balance_minor
            FROM sandbox_wallets AS wallet
            LEFT JOIN ledger_entries AS entry
                ON entry.account_id = wallet.ledger_account_id
            WHERE wallet.identity_id = $1
            GROUP BY
                wallet.id,
                wallet.identity_id,
                wallet.ledger_account_id,
                wallet.currency,
                wallet.created_at_epoch_seconds
            "#,
        )
        .bind(identity_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| database_error("load sandbox wallet", error))?
        .ok_or_else(|| {
            ApiError::not_found(
                "WALLET_NOT_FOUND",
                "sandbox wallet has not been created for this identity",
            )
        })?;

        row.into_view()
    }

    pub(super) async fn credit_wallet(
        &self,
        identity_id: Uuid,
        idempotency_key: &str,
        amount_minor_units: &str,
        now: u64,
    ) -> Result<SandboxCreditResponse, ApiError> {
        let idempotency_key = validate_idempotency_key(idempotency_key)?;
        let amount_minor = parse_positive_minor_units(amount_minor_units)?;
        let now_database = to_database_epoch(now)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| database_error("begin sandbox credit", error))?;

        let wallet = sqlx::query_as::<_, LockedWalletRow>(
            r#"
            SELECT
                wallet.id,
                wallet.identity_id,
                wallet.ledger_account_id,
                wallet.currency,
                identity.country_code
            FROM sandbox_wallets AS wallet
            INNER JOIN sandbox_identities AS identity
                ON identity.id = wallet.identity_id
            WHERE wallet.identity_id = $1
            FOR UPDATE OF wallet
            "#,
        )
        .bind(identity_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| database_error("lock sandbox wallet", error))?
        .ok_or_else(|| {
            ApiError::not_found(
                "WALLET_NOT_FOUND",
                "sandbox wallet has not been created for this identity",
            )
        })?;

        let limit = SandboxStore::limits_for_country(&wallet.country_code)
            .per_operation_minor_units
            .parse::<i64>()
            .map_err(|_| ApiError::internal("Pay Limits returned an invalid amount"))?;
        if amount_minor > limit {
            return Err(ApiError::bad_request(
                "PAY_LIMIT_EXCEEDED",
                format!(
                    "sandbox credit exceeds the per-operation limit of {limit} {} minor units",
                    wallet.currency
                ),
            ));
        }

        let fingerprint = digest_token(&format!(
            "{SANDBOX_CREDIT_KIND}|{identity_id}|{}|{}|{amount_minor}",
            wallet.id, wallet.currency
        ));

        if let Some(existing) =
            load_credit_by_idempotency_key(&mut transaction, idempotency_key).await?
        {
            return replay_credit(existing, &fingerprint, wallet.id);
        }

        let current_balance = wallet_balance(&mut transaction, wallet.ledger_account_id).await?;
        let resulting_balance = current_balance
            .checked_add(amount_minor)
            .ok_or_else(|| ApiError::bad_request("AMOUNT_OVERFLOW", "wallet balance overflow"))?;
        let transaction_id = Uuid::new_v4();

        let inserted_transaction_id = sqlx::query_scalar::<_, Uuid>(
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
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (idempotency_key) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(transaction_id)
        .bind(SANDBOX_CREDIT_KIND)
        .bind(&wallet.currency)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(resulting_balance)
        .bind(now_database)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| database_error("insert sandbox credit transaction", error))?;

        if inserted_transaction_id.is_none() {
            let existing = load_credit_by_idempotency_key(&mut transaction, idempotency_key)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "IDEMPOTENCY_CONFLICT",
                        "Idempotency-Key was already used by another operation",
                    )
                })?;
            return replay_credit(existing, &fingerprint, wallet.id);
        }

        let funding_account_code = format!("sandbox_funding:{}", wallet.currency);
        let funding_account_id: Uuid =
            sqlx::query_scalar("SELECT id FROM ledger_accounts WHERE account_code = $1")
                .bind(&funding_account_code)
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| database_error("load sandbox funding account", error))?;

        insert_entry(
            &mut transaction,
            transaction_id,
            funding_account_id,
            "debit",
            amount_minor,
            now_database,
        )
        .await?;
        insert_entry(
            &mut transaction,
            transaction_id,
            wallet.ledger_account_id,
            "credit",
            amount_minor,
            now_database,
        )
        .await?;

        transaction
            .commit()
            .await
            .map_err(|error| database_error("commit sandbox credit", error))?;

        Ok(SandboxCreditResponse {
            transaction_id,
            wallet_id: wallet.id,
            transaction_kind: SANDBOX_CREDIT_KIND.to_owned(),
            currency: wallet.currency,
            amount_minor_units: amount_minor.to_string(),
            balance_after_minor_units: resulting_balance.to_string(),
            posted_at_epoch_seconds: now,
        })
    }

    pub(super) async fn transfer_wallet(
        &self,
        sender_identity_id: Uuid,
        recipient_identity_id: Uuid,
        idempotency_key: &str,
        amount_minor_units: &str,
        now: u64,
    ) -> Result<SandboxTransferResponse, ApiError> {
        if sender_identity_id == recipient_identity_id {
            return Err(ApiError::bad_request(
                "SELF_TRANSFER_NOT_ALLOWED",
                "sender and recipient must be different identities",
            ));
        }

        let idempotency_key = validate_idempotency_key(idempotency_key)?;
        let amount_minor = parse_positive_minor_units(amount_minor_units)?;
        let now_database = to_database_epoch(now)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| database_error("begin sandbox P2P transfer", error))?;

        let (first_identity_id, second_identity_id) = if sender_identity_id < recipient_identity_id
        {
            (sender_identity_id, recipient_identity_id)
        } else {
            (recipient_identity_id, sender_identity_id)
        };

        let first_wallet = lock_wallet_by_identity(&mut transaction, first_identity_id)
            .await?
            .ok_or_else(|| missing_transfer_wallet(first_identity_id, sender_identity_id))?;
        let second_wallet = lock_wallet_by_identity(&mut transaction, second_identity_id)
            .await?
            .ok_or_else(|| missing_transfer_wallet(second_identity_id, sender_identity_id))?;

        let (sender, recipient) = if first_wallet.identity_id == sender_identity_id {
            (first_wallet, second_wallet)
        } else {
            (second_wallet, first_wallet)
        };

        if sender.currency != recipient.currency {
            return Err(ApiError::conflict(
                "CURRENCY_MISMATCH",
                "sandbox P2P transfers require both wallets to use the same currency",
            ));
        }

        let limit = SandboxStore::limits_for_country(&sender.country_code)
            .per_operation_minor_units
            .parse::<i64>()
            .map_err(|_| ApiError::internal("Pay Limits returned an invalid amount"))?;
        if amount_minor > limit {
            return Err(ApiError::bad_request(
                "PAY_LIMIT_EXCEEDED",
                format!(
                    "sandbox transfer exceeds the per-operation limit of {limit} {} minor units",
                    sender.currency
                ),
            ));
        }

        let fingerprint = digest_token(&format!(
            "{SANDBOX_P2P_TRANSFER_KIND}|{sender_identity_id}|{recipient_identity_id}|{}|{}|{}|{amount_minor}",
            sender.id, recipient.id, sender.currency
        ));

        if let Some(existing) =
            load_transfer_by_idempotency_key(&mut transaction, idempotency_key).await?
        {
            return replay_transfer(existing, &fingerprint, sender.id, recipient.id);
        }

        let sender_balance = wallet_balance(&mut transaction, sender.ledger_account_id).await?;
        if sender_balance < amount_minor {
            return Err(ApiError::conflict(
                "INSUFFICIENT_FUNDS",
                format!(
                    "sandbox wallet balance is {sender_balance} {} minor units, below the requested {amount_minor}",
                    sender.currency
                ),
            ));
        }

        let recipient_balance =
            wallet_balance(&mut transaction, recipient.ledger_account_id).await?;
        let sender_balance_after = sender_balance.checked_sub(amount_minor).ok_or_else(|| {
            ApiError::internal("sender balance subtraction failed after sufficient-funds check")
        })?;
        let recipient_balance_after =
            recipient_balance.checked_add(amount_minor).ok_or_else(|| {
                ApiError::bad_request("AMOUNT_OVERFLOW", "recipient balance overflow")
            })?;
        let transaction_id = Uuid::new_v4();

        let inserted_transaction_id = sqlx::query_scalar::<_, Uuid>(
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
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (idempotency_key) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(transaction_id)
        .bind(SANDBOX_P2P_TRANSFER_KIND)
        .bind(&sender.currency)
        .bind(idempotency_key)
        .bind(&fingerprint)
        .bind(sender_balance_after)
        .bind(now_database)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| database_error("insert sandbox P2P ledger transaction", error))?;

        if inserted_transaction_id.is_none() {
            let existing = load_transfer_by_idempotency_key(&mut transaction, idempotency_key)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "IDEMPOTENCY_CONFLICT",
                        "Idempotency-Key was already used by another operation",
                    )
                })?;
            return replay_transfer(existing, &fingerprint, sender.id, recipient.id);
        }

        sqlx::query(
            r#"
            INSERT INTO sandbox_p2p_transfers (
                transaction_id,
                sender_wallet_id,
                recipient_wallet_id,
                amount_minor,
                sender_balance_after_minor,
                recipient_balance_after_minor,
                created_at_epoch_seconds
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(transaction_id)
        .bind(sender.id)
        .bind(recipient.id)
        .bind(amount_minor)
        .bind(sender_balance_after)
        .bind(recipient_balance_after)
        .bind(now_database)
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("insert sandbox P2P metadata", error))?;

        insert_entry(
            &mut transaction,
            transaction_id,
            sender.ledger_account_id,
            "debit",
            amount_minor,
            now_database,
        )
        .await?;
        insert_entry(
            &mut transaction,
            transaction_id,
            recipient.ledger_account_id,
            "credit",
            amount_minor,
            now_database,
        )
        .await?;

        transaction
            .commit()
            .await
            .map_err(|error| database_error("commit sandbox P2P transfer", error))?;

        Ok(SandboxTransferResponse {
            transaction_id,
            transaction_kind: SANDBOX_P2P_TRANSFER_KIND.to_owned(),
            sender_wallet_id: sender.id,
            recipient_wallet_id: recipient.id,
            currency: sender.currency,
            amount_minor_units: amount_minor.to_string(),
            sender_balance_after_minor_units: sender_balance_after.to_string(),
            recipient_balance_after_minor_units: recipient_balance_after.to_string(),
            posted_at_epoch_seconds: now,
        })
    }
}

impl SandboxStore {
    pub async fn transfer_wallet(
        &self,
        sender_identity_id: Uuid,
        recipient_identity_id: Uuid,
        idempotency_key: &str,
        amount_minor_units: &str,
        now: u64,
    ) -> Result<SandboxTransferResponse, ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Err(ApiError::service_unavailable(
                "DATABASE_REQUIRED",
                "sandbox P2P transfers require the PostgreSQL backend",
            )),
            StoreBackend::Postgres { ledger, .. } => {
                ledger
                    .transfer_wallet(
                        sender_identity_id,
                        recipient_identity_id,
                        idempotency_key,
                        amount_minor_units,
                        now,
                    )
                    .await
            }
        }
    }
}

impl WalletRow {
    fn into_view(self) -> Result<WalletView, ApiError> {
        let _ledger_account_id = self.ledger_account_id;
        Ok(WalletView {
            id: self.id,
            identity_id: self.identity_id,
            currency: self.currency,
            balance_minor_units: self.balance_minor.to_string(),
            created_at_epoch_seconds: from_database_epoch(self.created_at_epoch_seconds)?,
        })
    }
}

async fn ensure_account(
    transaction: &mut Transaction<'_, Postgres>,
    account_code: &str,
    account_class: &str,
    normal_side: &str,
    currency: &str,
    now: i64,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO ledger_accounts (
            id,
            account_code,
            account_class,
            normal_side,
            currency,
            created_at_epoch_seconds
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (account_code) DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(account_code)
    .bind(account_class)
    .bind(normal_side)
    .bind(currency)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|error| database_error("ensure ledger account", error))?;

    Ok(())
}

async fn lock_wallet_by_identity(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: Uuid,
) -> Result<Option<LockedWalletRow>, ApiError> {
    sqlx::query_as::<_, LockedWalletRow>(
        r#"
        SELECT
            wallet.id,
            wallet.identity_id,
            wallet.ledger_account_id,
            wallet.currency,
            identity.country_code
        FROM sandbox_wallets AS wallet
        INNER JOIN sandbox_identities AS identity
            ON identity.id = wallet.identity_id
        WHERE wallet.identity_id = $1
        FOR UPDATE OF wallet
        "#,
    )
    .bind(identity_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| database_error("lock sandbox transfer wallet", error))
}

fn missing_transfer_wallet(identity_id: Uuid, sender_identity_id: Uuid) -> ApiError {
    if identity_id == sender_identity_id {
        ApiError::not_found(
            "WALLET_NOT_FOUND",
            "sender sandbox wallet has not been created",
        )
    } else {
        ApiError::not_found(
            "RECIPIENT_WALLET_NOT_FOUND",
            "recipient sandbox wallet has not been created",
        )
    }
}

async fn wallet_balance(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<i64, ApiError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(
            SUM(
                CASE entry_side
                    WHEN 'credit' THEN amount_minor
                    WHEN 'debit' THEN -amount_minor
                END
            ),
            0
        )::BIGINT
        FROM ledger_entries
        WHERE account_id = $1
        "#,
    )
    .bind(account_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| database_error("calculate wallet balance", error))
}

async fn insert_entry(
    transaction: &mut Transaction<'_, Postgres>,
    transaction_id: Uuid,
    account_id: Uuid,
    entry_side: &str,
    amount_minor: i64,
    now: i64,
) -> Result<(), ApiError> {
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
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(transaction_id)
    .bind(account_id)
    .bind(entry_side)
    .bind(amount_minor)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .map_err(|error| database_error("insert ledger entry", error))?;

    Ok(())
}

async fn load_credit_by_idempotency_key(
    transaction: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<CreditReplayRow>, ApiError> {
    sqlx::query_as::<_, CreditReplayRow>(
        r#"
        SELECT
            transaction.id AS transaction_id,
            wallet.id AS wallet_id,
            transaction.transaction_kind,
            transaction.currency,
            entry.amount_minor,
            transaction.resulting_balance_minor,
            transaction.posted_at_epoch_seconds,
            transaction.request_fingerprint
        FROM ledger_transactions AS transaction
        INNER JOIN ledger_entries AS entry
            ON entry.transaction_id = transaction.id
           AND entry.entry_side = 'credit'
        INNER JOIN sandbox_wallets AS wallet
            ON wallet.ledger_account_id = entry.account_id
        WHERE transaction.idempotency_key = $1
          AND transaction.transaction_kind = $2
        "#,
    )
    .bind(idempotency_key)
    .bind(SANDBOX_CREDIT_KIND)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| database_error("load idempotent sandbox credit", error))
}

async fn load_transfer_by_idempotency_key(
    transaction: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<TransferReplayRow>, ApiError> {
    sqlx::query_as::<_, TransferReplayRow>(
        r#"
        SELECT
            transaction.id AS transaction_id,
            transaction.transaction_kind,
            transfer.sender_wallet_id,
            transfer.recipient_wallet_id,
            transaction.currency,
            transfer.amount_minor,
            transfer.sender_balance_after_minor,
            transfer.recipient_balance_after_minor,
            transaction.posted_at_epoch_seconds,
            transaction.request_fingerprint
        FROM ledger_transactions AS transaction
        INNER JOIN sandbox_p2p_transfers AS transfer
            ON transfer.transaction_id = transaction.id
        WHERE transaction.idempotency_key = $1
          AND transaction.transaction_kind = $2
        "#,
    )
    .bind(idempotency_key)
    .bind(SANDBOX_P2P_TRANSFER_KIND)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| database_error("load idempotent sandbox P2P transfer", error))
}

fn replay_credit(
    existing: CreditReplayRow,
    expected_fingerprint: &str,
    expected_wallet_id: Uuid,
) -> Result<SandboxCreditResponse, ApiError> {
    if existing.request_fingerprint != expected_fingerprint
        || existing.wallet_id != expected_wallet_id
    {
        return Err(ApiError::conflict(
            "IDEMPOTENCY_CONFLICT",
            "Idempotency-Key was already used with a different sandbox credit request",
        ));
    }

    Ok(SandboxCreditResponse {
        transaction_id: existing.transaction_id,
        wallet_id: existing.wallet_id,
        transaction_kind: existing.transaction_kind,
        currency: existing.currency,
        amount_minor_units: existing.amount_minor.to_string(),
        balance_after_minor_units: existing.resulting_balance_minor.to_string(),
        posted_at_epoch_seconds: from_database_epoch(existing.posted_at_epoch_seconds)?,
    })
}

fn replay_transfer(
    existing: TransferReplayRow,
    expected_fingerprint: &str,
    expected_sender_wallet_id: Uuid,
    expected_recipient_wallet_id: Uuid,
) -> Result<SandboxTransferResponse, ApiError> {
    if existing.request_fingerprint != expected_fingerprint
        || existing.sender_wallet_id != expected_sender_wallet_id
        || existing.recipient_wallet_id != expected_recipient_wallet_id
    {
        return Err(ApiError::conflict(
            "IDEMPOTENCY_CONFLICT",
            "Idempotency-Key was already used with a different sandbox transfer request",
        ));
    }

    Ok(SandboxTransferResponse {
        transaction_id: existing.transaction_id,
        transaction_kind: existing.transaction_kind,
        sender_wallet_id: existing.sender_wallet_id,
        recipient_wallet_id: existing.recipient_wallet_id,
        currency: existing.currency,
        amount_minor_units: existing.amount_minor.to_string(),
        sender_balance_after_minor_units: existing.sender_balance_after_minor.to_string(),
        recipient_balance_after_minor_units: existing.recipient_balance_after_minor.to_string(),
        posted_at_epoch_seconds: from_database_epoch(existing.posted_at_epoch_seconds)?,
    })
}

fn validate_idempotency_key(value: &str) -> Result<&str, ApiError> {
    if value.trim() != value
        || !(8..=128).contains(&value.len())
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request(
            "IDEMPOTENCY_KEY_INVALID",
            "Idempotency-Key must contain 8 to 128 visible characters without surrounding whitespace",
        ));
    }

    Ok(value)
}

fn parse_positive_minor_units(value: &str) -> Result<i64, ApiError> {
    if value.trim() != value || value.is_empty() {
        return Err(ApiError::bad_request(
            "AMOUNT_INVALID",
            "amount_minor_units must be a positive base-10 integer string",
        ));
    }

    let amount = value.parse::<i64>().map_err(|_| {
        ApiError::bad_request(
            "AMOUNT_INVALID",
            "amount_minor_units must be a positive base-10 integer string",
        )
    })?;
    if amount <= 0 {
        return Err(ApiError::bad_request(
            "AMOUNT_INVALID",
            "amount_minor_units must be greater than zero",
        ));
    }

    Ok(amount)
}

fn home_currency(country_code: &str) -> &'static str {
    match country_code {
        "PE" => "PEN",
        "BR" => "BRL",
        "MX" => "MXN",
        "CO" => "COP",
        _ => "USD",
    }
}

fn to_database_epoch(value: u64) -> Result<i64, ApiError> {
    i64::try_from(value).map_err(|_| ApiError::internal("epoch value exceeds PostgreSQL BIGINT"))
}

fn from_database_epoch(value: i64) -> Result<u64, ApiError> {
    u64::try_from(value).map_err(|_| ApiError::internal("stored epoch value is negative"))
}

fn database_error(context: &'static str, error_value: sqlx::Error) -> ApiError {
    error!(context, error = %error_value, "PostgreSQL wallet operation failed");
    ApiError::internal("wallet persistence operation failed")
}
