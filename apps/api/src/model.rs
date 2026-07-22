use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateIdentityRequest {
    pub email: String,
    pub display_name: String,
    pub country_code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityView {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub country_code: String,
    pub pin_configured: bool,
    pub created_at_epoch_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub identity_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_at_epoch_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct PinRequest {
    pub pin: String,
}

#[derive(Debug, Serialize)]
pub struct PinVerificationResponse {
    pub verified: bool,
    pub remaining_attempts: u8,
    pub locked_until_epoch_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PayLimitsResponse {
    pub module: &'static str,
    pub environment: &'static str,
    pub currency: String,
    pub per_operation_minor_units: String,
    pub daily_minor_units: String,
    pub monthly_minor_units: String,
    pub payments_enabled: bool,
    pub transfers_enabled: bool,
    pub kyc_tier: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletView {
    pub id: Uuid,
    pub identity_id: Uuid,
    pub currency: String,
    pub balance_minor_units: String,
    pub created_at_epoch_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct SandboxCreditRequest {
    pub amount_minor_units: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxCreditResponse {
    pub transaction_id: Uuid,
    pub wallet_id: Uuid,
    pub transaction_kind: String,
    pub currency: String,
    pub amount_minor_units: String,
    pub balance_after_minor_units: String,
    pub posted_at_epoch_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct SandboxTransferRequest {
    pub recipient_identity_id: Uuid,
    pub amount_minor_units: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxTransferResponse {
    pub transaction_id: Uuid,
    pub transaction_kind: String,
    pub sender_wallet_id: Uuid,
    pub recipient_wallet_id: Uuid,
    pub currency: String,
    pub amount_minor_units: String,
    pub sender_balance_after_minor_units: String,
    pub recipient_balance_after_minor_units: String,
    pub posted_at_epoch_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct PayActivityQuery {
    pub limit: Option<u16>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PayActivityCounterparty {
    pub identity_id: Uuid,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PayActivityItem {
    pub transaction_id: Uuid,
    pub transaction_kind: String,
    pub wallet_id: Uuid,
    pub direction: String,
    pub currency: String,
    pub amount_minor_units: String,
    pub balance_after_minor_units: String,
    pub counterparty: Option<PayActivityCounterparty>,
    pub posted_at_epoch_seconds: u64,
    pub receipt_available: bool,
}

#[derive(Debug, Serialize)]
pub struct PayActivityPage {
    pub module: &'static str,
    pub environment: &'static str,
    pub items: Vec<PayActivityItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PayReceiptResponse {
    pub module: &'static str,
    pub environment: &'static str,
    pub receipt_version: u8,
    pub receipt_reference: String,
    pub transaction_id: Uuid,
    pub transaction_kind: String,
    pub status: &'static str,
    pub direction: String,
    pub wallet_id: Uuid,
    pub counterparty: Option<PayActivityCounterparty>,
    pub currency: String,
    pub amount_minor_units: String,
    pub balance_after_minor_units: String,
    pub posted_at_epoch_seconds: u64,
    pub ledger_entry_count: i64,
    pub ledger_debit_total_minor_units: String,
    pub ledger_credit_total_minor_units: String,
}
