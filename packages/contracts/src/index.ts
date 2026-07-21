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
