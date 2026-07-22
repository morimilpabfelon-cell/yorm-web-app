export const yormModules = [
  'Yorm Pay',
  'Compliance Layer',
  'Pay Limits',
  'Pay Convert',
  'Pay Exchange Link',
  'Pay QR',
  'Pay Code',
  'Pay Link',
  'Pay Merchant',
  'Pay Touch',
  'Pay Card',
  'Pay Disposable Card',
  'Pay Checkout',
  'Pay Payouts',
  'Pay Gateway',
  'Pay Receipt',
  'Pay Activity',
  'Pay Guide',
  'Pay Safe',
  'Pay Card Liquidity',
] as const;

export type YormModule = (typeof yormModules)[number];

export type Environment = 'local' | 'test' | 'sandbox' | 'production';

export interface SystemStatus {
  readonly service: 'yorm-api';
  readonly environment: Environment;
  readonly version: string;
  readonly real_money_enabled: boolean;
  readonly external_providers_enabled: boolean;
}

export interface Money {
  readonly currency: string;
  readonly minorUnits: bigint;
}

export interface CreateIdentityRequest {
  readonly email: string;
  readonly display_name: string;
  readonly country_code: string;
}

export interface IdentityView {
  readonly id: string;
  readonly email: string;
  readonly display_name: string;
  readonly country_code: string;
  readonly pin_configured: boolean;
  readonly created_at_epoch_seconds: number;
}

export interface CreateSessionRequest {
  readonly identity_id: string;
}

export interface SessionResponse {
  readonly access_token: string;
  readonly token_type: 'Bearer';
  readonly expires_at_epoch_seconds: number;
}

export interface PinRequest {
  readonly pin: string;
}

export interface PinVerificationResponse {
  readonly verified: boolean;
  readonly remaining_attempts: number;
  readonly locked_until_epoch_seconds: number | null;
}

export interface PayLimitsResponse {
  readonly module: 'Pay Limits';
  readonly environment: 'sandbox';
  readonly currency: string;
  readonly per_operation_minor_units: string;
  readonly daily_minor_units: string;
  readonly monthly_minor_units: string;
  readonly payments_enabled: false;
  readonly transfers_enabled: false;
  readonly kyc_tier: 'sandbox_unverified';
}

export interface WalletView {
  readonly id: string;
  readonly identity_id: string;
  readonly currency: string;
  readonly balance_minor_units: string;
  readonly created_at_epoch_seconds: number;
}

export interface SandboxCreditRequest {
  readonly amount_minor_units: string;
}

export interface SandboxCreditResponse {
  readonly transaction_id: string;
  readonly wallet_id: string;
  readonly transaction_kind: 'sandbox_credit';
  readonly currency: string;
  readonly amount_minor_units: string;
  readonly balance_after_minor_units: string;
  readonly posted_at_epoch_seconds: number;
}

export interface SandboxTransferRequest {
  readonly recipient_identity_id: string;
  readonly amount_minor_units: string;
}

export interface SandboxTransferResponse {
  readonly transaction_id: string;
  readonly transaction_kind: 'sandbox_p2p_transfer';
  readonly sender_wallet_id: string;
  readonly recipient_wallet_id: string;
  readonly currency: string;
  readonly amount_minor_units: string;
  readonly sender_balance_after_minor_units: string;
  readonly recipient_balance_after_minor_units: string;
  readonly posted_at_epoch_seconds: number;
}

export interface ApiErrorResponse {
  readonly error: {
    readonly code: string;
    readonly message: string;
  };
}
