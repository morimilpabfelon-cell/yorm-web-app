CREATE TABLE ledger_accounts (
    id UUID PRIMARY KEY,
    account_code TEXT NOT NULL UNIQUE,
    account_class TEXT NOT NULL,
    normal_side TEXT NOT NULL,
    currency CHAR(3) NOT NULL,
    created_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT ledger_accounts_code_format
        CHECK (account_code ~ '^[A-Za-z0-9:_-]{3,160}$'),
    CONSTRAINT ledger_accounts_class
        CHECK (account_class IN ('asset', 'liability')),
    CONSTRAINT ledger_accounts_normal_side
        CHECK (normal_side IN ('debit', 'credit')),
    CONSTRAINT ledger_accounts_class_side_consistency
        CHECK (
            (account_class = 'asset' AND normal_side = 'debit')
            OR (account_class = 'liability' AND normal_side = 'credit')
        ),
    CONSTRAINT ledger_accounts_currency_format
        CHECK (currency ~ '^[A-Z]{3}$'),
    CONSTRAINT ledger_accounts_created_at_nonnegative
        CHECK (created_at_epoch_seconds >= 0)
);

CREATE TABLE sandbox_wallets (
    id UUID PRIMARY KEY,
    identity_id UUID NOT NULL
        REFERENCES sandbox_identities(id)
        ON DELETE RESTRICT,
    ledger_account_id UUID NOT NULL UNIQUE
        REFERENCES ledger_accounts(id)
        ON DELETE RESTRICT,
    currency CHAR(3) NOT NULL,
    created_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT sandbox_wallets_identity_currency_unique
        UNIQUE (identity_id, currency),
    CONSTRAINT sandbox_wallets_currency_format
        CHECK (currency ~ '^[A-Z]{3}$'),
    CONSTRAINT sandbox_wallets_created_at_nonnegative
        CHECK (created_at_epoch_seconds >= 0)
);

CREATE TABLE ledger_transactions (
    id UUID PRIMARY KEY,
    transaction_kind TEXT NOT NULL,
    currency CHAR(3) NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    request_fingerprint TEXT NOT NULL,
    resulting_balance_minor BIGINT NOT NULL,
    posted_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT ledger_transactions_kind_format
        CHECK (transaction_kind ~ '^[a-z][a-z0-9_]{2,63}$'),
    CONSTRAINT ledger_transactions_currency_format
        CHECK (currency ~ '^[A-Z]{3}$'),
    CONSTRAINT ledger_transactions_idempotency_length
        CHECK (char_length(idempotency_key) BETWEEN 8 AND 128),
    CONSTRAINT ledger_transactions_fingerprint_length
        CHECK (char_length(request_fingerprint) = 43),
    CONSTRAINT ledger_transactions_resulting_balance_nonnegative
        CHECK (resulting_balance_minor >= 0),
    CONSTRAINT ledger_transactions_posted_at_nonnegative
        CHECK (posted_at_epoch_seconds >= 0)
);

CREATE TABLE ledger_entries (
    id UUID PRIMARY KEY,
    transaction_id UUID NOT NULL
        REFERENCES ledger_transactions(id)
        ON DELETE RESTRICT,
    account_id UUID NOT NULL
        REFERENCES ledger_accounts(id)
        ON DELETE RESTRICT,
    entry_side TEXT NOT NULL,
    amount_minor BIGINT NOT NULL,
    created_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT ledger_entries_side
        CHECK (entry_side IN ('debit', 'credit')),
    CONSTRAINT ledger_entries_amount_positive
        CHECK (amount_minor > 0),
    CONSTRAINT ledger_entries_created_at_nonnegative
        CHECK (created_at_epoch_seconds >= 0)
);

CREATE INDEX ledger_entries_transaction_id_idx
    ON ledger_entries(transaction_id);

CREATE INDEX ledger_entries_account_id_idx
    ON ledger_entries(account_id, created_at_epoch_seconds, id);

CREATE INDEX ledger_transactions_posted_at_idx
    ON ledger_transactions(posted_at_epoch_seconds, id);

CREATE OR REPLACE FUNCTION reject_immutable_ledger_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION USING
        ERRCODE = '55000',
        MESSAGE = format('%s is immutable after insertion', TG_TABLE_NAME);
END;
$$;

CREATE TRIGGER ledger_accounts_immutable
BEFORE UPDATE OR DELETE ON ledger_accounts
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_ledger_mutation();

CREATE TRIGGER sandbox_wallets_immutable
BEFORE UPDATE OR DELETE ON sandbox_wallets
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_ledger_mutation();

CREATE TRIGGER ledger_transactions_immutable
BEFORE UPDATE OR DELETE ON ledger_transactions
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_ledger_mutation();

CREATE TRIGGER ledger_entries_immutable
BEFORE UPDATE OR DELETE ON ledger_entries
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_ledger_mutation();

CREATE OR REPLACE FUNCTION validate_ledger_transaction_balance()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    target_transaction_id UUID;
    entry_count BIGINT;
    debit_total NUMERIC;
    credit_total NUMERIC;
    currency_mismatch_count BIGINT;
BEGIN
    IF TG_TABLE_NAME = 'ledger_transactions' THEN
        target_transaction_id := COALESCE(NEW.id, OLD.id);
    ELSE
        target_transaction_id := COALESCE(NEW.transaction_id, OLD.transaction_id);
    END IF;

    SELECT
        COUNT(*),
        COALESCE(SUM(entry.amount_minor) FILTER (WHERE entry.entry_side = 'debit'), 0),
        COALESCE(SUM(entry.amount_minor) FILTER (WHERE entry.entry_side = 'credit'), 0),
        COUNT(*) FILTER (WHERE account.currency <> transaction.currency)
    INTO
        entry_count,
        debit_total,
        credit_total,
        currency_mismatch_count
    FROM ledger_entries AS entry
    INNER JOIN ledger_accounts AS account
        ON account.id = entry.account_id
    INNER JOIN ledger_transactions AS transaction
        ON transaction.id = entry.transaction_id
    WHERE entry.transaction_id = target_transaction_id;

    IF entry_count < 2 THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = format(
                'ledger transaction %s must contain at least two entries',
                target_transaction_id
            );
    END IF;

    IF debit_total <> credit_total THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = format(
                'ledger transaction %s is unbalanced: debits=%s credits=%s',
                target_transaction_id,
                debit_total,
                credit_total
            );
    END IF;

    IF currency_mismatch_count <> 0 THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = format(
                'ledger transaction %s contains an account with a different currency',
                target_transaction_id
            );
    END IF;

    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER ledger_transactions_require_balanced_entries
AFTER INSERT ON ledger_transactions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION validate_ledger_transaction_balance();

CREATE CONSTRAINT TRIGGER ledger_entries_preserve_balance
AFTER INSERT OR UPDATE OR DELETE ON ledger_entries
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION validate_ledger_transaction_balance();
