CREATE TABLE sandbox_p2p_transfers (
    transaction_id UUID PRIMARY KEY
        REFERENCES ledger_transactions(id)
        ON DELETE RESTRICT,
    sender_wallet_id UUID NOT NULL
        REFERENCES sandbox_wallets(id)
        ON DELETE RESTRICT,
    recipient_wallet_id UUID NOT NULL
        REFERENCES sandbox_wallets(id)
        ON DELETE RESTRICT,
    amount_minor BIGINT NOT NULL,
    sender_balance_after_minor BIGINT NOT NULL,
    recipient_balance_after_minor BIGINT NOT NULL,
    created_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT sandbox_p2p_transfers_distinct_wallets
        CHECK (sender_wallet_id <> recipient_wallet_id),
    CONSTRAINT sandbox_p2p_transfers_amount_positive
        CHECK (amount_minor > 0),
    CONSTRAINT sandbox_p2p_transfers_sender_balance_nonnegative
        CHECK (sender_balance_after_minor >= 0),
    CONSTRAINT sandbox_p2p_transfers_recipient_balance_nonnegative
        CHECK (recipient_balance_after_minor >= 0),
    CONSTRAINT sandbox_p2p_transfers_created_at_nonnegative
        CHECK (created_at_epoch_seconds >= 0)
);

CREATE INDEX sandbox_p2p_transfers_sender_idx
    ON sandbox_p2p_transfers(sender_wallet_id, created_at_epoch_seconds, transaction_id);

CREATE INDEX sandbox_p2p_transfers_recipient_idx
    ON sandbox_p2p_transfers(recipient_wallet_id, created_at_epoch_seconds, transaction_id);

CREATE TRIGGER sandbox_p2p_transfers_immutable
BEFORE UPDATE OR DELETE ON sandbox_p2p_transfers
FOR EACH ROW
EXECUTE FUNCTION reject_immutable_ledger_mutation();

CREATE OR REPLACE FUNCTION validate_sandbox_p2p_transfer()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    transaction_row_count BIGINT;
    total_entry_count BIGINT;
    matching_entry_count BIGINT;
    sender_balance BIGINT;
    recipient_balance BIGINT;
BEGIN
    SELECT COUNT(*)
    INTO transaction_row_count
    FROM ledger_transactions AS transaction
    INNER JOIN sandbox_wallets AS sender
        ON sender.id = NEW.sender_wallet_id
    INNER JOIN sandbox_wallets AS recipient
        ON recipient.id = NEW.recipient_wallet_id
    WHERE transaction.id = NEW.transaction_id
      AND transaction.transaction_kind = 'sandbox_p2p_transfer'
      AND transaction.currency = sender.currency
      AND transaction.currency = recipient.currency
      AND transaction.resulting_balance_minor = NEW.sender_balance_after_minor;

    IF transaction_row_count <> 1 THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = format(
                'sandbox P2P transfer %s metadata does not match its ledger transaction',
                NEW.transaction_id
            );
    END IF;

    SELECT
        COUNT(*),
        COUNT(*) FILTER (
            WHERE (
                entry.account_id = sender.ledger_account_id
                AND entry.entry_side = 'debit'
                AND entry.amount_minor = NEW.amount_minor
            )
            OR (
                entry.account_id = recipient.ledger_account_id
                AND entry.entry_side = 'credit'
                AND entry.amount_minor = NEW.amount_minor
            )
        )
    INTO total_entry_count, matching_entry_count
    FROM ledger_entries AS entry
    CROSS JOIN sandbox_wallets AS sender
    CROSS JOIN sandbox_wallets AS recipient
    WHERE entry.transaction_id = NEW.transaction_id
      AND sender.id = NEW.sender_wallet_id
      AND recipient.id = NEW.recipient_wallet_id;

    IF total_entry_count <> 2 OR matching_entry_count <> 2 THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = format(
                'sandbox P2P transfer %s must contain exactly one sender debit and one recipient credit',
                NEW.transaction_id
            );
    END IF;

    SELECT COALESCE(
        SUM(
            CASE entry.entry_side
                WHEN 'credit' THEN entry.amount_minor
                WHEN 'debit' THEN -entry.amount_minor
            END
        ),
        0
    )::BIGINT
    INTO sender_balance
    FROM sandbox_wallets AS wallet
    LEFT JOIN ledger_entries AS entry
        ON entry.account_id = wallet.ledger_account_id
    WHERE wallet.id = NEW.sender_wallet_id;

    SELECT COALESCE(
        SUM(
            CASE entry.entry_side
                WHEN 'credit' THEN entry.amount_minor
                WHEN 'debit' THEN -entry.amount_minor
            END
        ),
        0
    )::BIGINT
    INTO recipient_balance
    FROM sandbox_wallets AS wallet
    LEFT JOIN ledger_entries AS entry
        ON entry.account_id = wallet.ledger_account_id
    WHERE wallet.id = NEW.recipient_wallet_id;

    IF sender_balance <> NEW.sender_balance_after_minor
       OR recipient_balance <> NEW.recipient_balance_after_minor THEN
        RAISE EXCEPTION USING
            ERRCODE = '23514',
            MESSAGE = format(
                'sandbox P2P transfer %s stored balances do not match the ledger',
                NEW.transaction_id
            );
    END IF;

    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER sandbox_p2p_transfers_validate
AFTER INSERT ON sandbox_p2p_transfers
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION validate_sandbox_p2p_transfer();
