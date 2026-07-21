CREATE TABLE sandbox_identities (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    country_code CHAR(2) NOT NULL,
    pin_hash TEXT,
    pin_failed_attempts SMALLINT NOT NULL DEFAULT 0,
    pin_locked_until_epoch_seconds BIGINT,
    created_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT sandbox_identities_email_normalized
        CHECK (email = lower(btrim(email))),
    CONSTRAINT sandbox_identities_display_name_length
        CHECK (char_length(display_name) BETWEEN 2 AND 80),
    CONSTRAINT sandbox_identities_country_code_format
        CHECK (country_code ~ '^[A-Z]{2}$'),
    CONSTRAINT sandbox_identities_pin_attempts_range
        CHECK (pin_failed_attempts BETWEEN 0 AND 5),
    CONSTRAINT sandbox_identities_created_at_nonnegative
        CHECK (created_at_epoch_seconds >= 0),
    CONSTRAINT sandbox_identities_pin_lock_nonnegative
        CHECK (
            pin_locked_until_epoch_seconds IS NULL
            OR pin_locked_until_epoch_seconds >= 0
        )
);

CREATE TABLE sandbox_sessions (
    token_digest TEXT PRIMARY KEY,
    identity_id UUID NOT NULL
        REFERENCES sandbox_identities(id)
        ON DELETE CASCADE,
    expires_at_epoch_seconds BIGINT NOT NULL,
    revoked_at_epoch_seconds BIGINT,
    created_at_epoch_seconds BIGINT NOT NULL,
    CONSTRAINT sandbox_sessions_digest_format
        CHECK (char_length(token_digest) BETWEEN 40 AND 64),
    CONSTRAINT sandbox_sessions_expiry_after_creation
        CHECK (expires_at_epoch_seconds > created_at_epoch_seconds),
    CONSTRAINT sandbox_sessions_revocation_nonnegative
        CHECK (
            revoked_at_epoch_seconds IS NULL
            OR revoked_at_epoch_seconds >= 0
        )
);

CREATE INDEX sandbox_sessions_identity_id_idx
    ON sandbox_sessions(identity_id);

CREATE INDEX sandbox_sessions_active_expiry_idx
    ON sandbox_sessions(expires_at_epoch_seconds)
    WHERE revoked_at_epoch_seconds IS NULL;
