use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};
use tracing::error;
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{IdentityView, PinVerificationResponse, SessionResponse},
};

use super::{
    AuthenticatedIdentity, MAX_PIN_ATTEMPTS, PIN_LOCK_SECONDS, SESSION_TTL_SECONDS,
    generate_session_token, incorrect_pin_error, normalize_country_code, normalize_display_name,
    normalize_email, pin_locked_after_failures_error, pin_locked_error, protect_pin,
    successful_pin_verification, validate_pin_format, verify_protected_pin,
};

#[derive(Clone)]
pub(super) struct PostgresStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct IdentityRow {
    id: Uuid,
    email: String,
    display_name: String,
    country_code: String,
    pin_hash: Option<String>,
    created_at_epoch_seconds: i64,
}

#[derive(sqlx::FromRow)]
struct SessionIdentityRow {
    identity_id: Uuid,
    email: String,
    display_name: String,
    country_code: String,
    pin_hash: Option<String>,
    created_at_epoch_seconds: i64,
    expires_at_epoch_seconds: i64,
    revoked_at_epoch_seconds: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct PinStateRow {
    pin_hash: Option<String>,
    pin_failed_attempts: i16,
    pin_locked_until_epoch_seconds: Option<i64>,
}

impl PostgresStore {
    pub(super) async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|migration_error| sqlx::Error::Protocol(migration_error.to_string()))?;

        Ok(Self { pool })
    }

    pub(super) async fn health(&self) -> Result<(), ApiError> {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| database_error("database healthcheck", error))
    }

    pub(super) async fn register_identity(
        &self,
        email: &str,
        display_name: &str,
        country_code: &str,
        now: u64,
    ) -> Result<IdentityView, ApiError> {
        let id = Uuid::new_v4();
        let email = normalize_email(email)?;
        let display_name = normalize_display_name(display_name)?;
        let country_code = normalize_country_code(country_code)?;
        let now = to_database_epoch(now)?;

        let result = sqlx::query_as::<_, IdentityRow>(
            r#"
            INSERT INTO sandbox_identities (
                id,
                email,
                display_name,
                country_code,
                created_at_epoch_seconds
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING
                id,
                email,
                display_name,
                country_code,
                pin_hash,
                created_at_epoch_seconds
            "#,
        )
        .bind(id)
        .bind(email)
        .bind(display_name)
        .bind(country_code)
        .bind(now)
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(row) => row.to_view(),
            Err(error) if is_unique_violation(&error) => Err(ApiError::conflict(
                "IDENTITY_ALREADY_EXISTS",
                "an identity with this email already exists",
            )),
            Err(error) => Err(database_error("register identity", error)),
        }
    }

    pub(super) async fn create_session(
        &self,
        identity_id: Uuid,
        now: u64,
    ) -> Result<SessionResponse, ApiError> {
        let (access_token, token_digest) = generate_session_token();
        let expires_at_epoch_seconds = now
            .checked_add(SESSION_TTL_SECONDS)
            .ok_or_else(|| ApiError::internal("session expiry overflow"))?;
        let now_database = to_database_epoch(now)?;
        let expiry_database = to_database_epoch(expires_at_epoch_seconds)?;

        let result = sqlx::query(
            r#"
            INSERT INTO sandbox_sessions (
                token_digest,
                identity_id,
                expires_at_epoch_seconds,
                created_at_epoch_seconds
            )
            SELECT $1, $2, $3, $4
            WHERE EXISTS (
                SELECT 1
                FROM sandbox_identities
                WHERE id = $2
            )
            "#,
        )
        .bind(token_digest)
        .bind(identity_id)
        .bind(expiry_database)
        .bind(now_database)
        .execute(&self.pool)
        .await
        .map_err(|error| database_error("create session", error))?;

        if result.rows_affected() == 0 {
            return Err(ApiError::not_found(
                "IDENTITY_NOT_FOUND",
                "identity does not exist",
            ));
        }

        Ok(SessionResponse {
            access_token,
            token_type: "Bearer",
            expires_at_epoch_seconds,
        })
    }

    pub(super) async fn authenticate(
        &self,
        access_token: &str,
        now: u64,
    ) -> Result<AuthenticatedIdentity, ApiError> {
        let token_digest = super::digest_token(access_token);
        let row = sqlx::query_as::<_, SessionIdentityRow>(
            r#"
            SELECT
                identity.id AS identity_id,
                identity.email,
                identity.display_name,
                identity.country_code,
                identity.pin_hash,
                identity.created_at_epoch_seconds,
                session.expires_at_epoch_seconds,
                session.revoked_at_epoch_seconds
            FROM sandbox_sessions AS session
            INNER JOIN sandbox_identities AS identity
                ON identity.id = session.identity_id
            WHERE session.token_digest = $1
            "#,
        )
        .bind(token_digest)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| database_error("authenticate session", error))?
        .ok_or_else(|| {
            ApiError::unauthorized("SESSION_INVALID", "session is invalid or revoked")
        })?;

        if row.revoked_at_epoch_seconds.is_some() {
            return Err(ApiError::unauthorized(
                "SESSION_INVALID",
                "session is invalid or revoked",
            ));
        }

        if from_database_epoch(row.expires_at_epoch_seconds)? <= now {
            return Err(ApiError::unauthorized(
                "SESSION_EXPIRED",
                "session has expired",
            ));
        }

        Ok(AuthenticatedIdentity {
            id: row.identity_id,
            view: IdentityView {
                id: row.identity_id,
                email: row.email,
                display_name: row.display_name,
                country_code: row.country_code,
                pin_configured: row.pin_hash.is_some(),
                created_at_epoch_seconds: from_database_epoch(row.created_at_epoch_seconds)?,
            },
        })
    }

    pub(super) async fn revoke_session(
        &self,
        access_token: &str,
        now: u64,
    ) -> Result<(), ApiError> {
        let result = sqlx::query(
            r#"
            UPDATE sandbox_sessions
            SET revoked_at_epoch_seconds = $2
            WHERE token_digest = $1
              AND revoked_at_epoch_seconds IS NULL
              AND expires_at_epoch_seconds > $2
            "#,
        )
        .bind(super::digest_token(access_token))
        .bind(to_database_epoch(now)?)
        .execute(&self.pool)
        .await
        .map_err(|error| database_error("revoke session", error))?;

        if result.rows_affected() == 0 {
            return Err(ApiError::unauthorized(
                "SESSION_INVALID",
                "session is invalid, expired, or revoked",
            ));
        }

        Ok(())
    }

    pub(super) async fn set_pin(&self, identity_id: Uuid, pin: &str) -> Result<(), ApiError> {
        let pin_hash = protect_pin(pin)?;
        let result = sqlx::query(
            r#"
            UPDATE sandbox_identities
            SET
                pin_hash = $2,
                pin_failed_attempts = 0,
                pin_locked_until_epoch_seconds = NULL
            WHERE id = $1
            "#,
        )
        .bind(identity_id)
        .bind(pin_hash)
        .execute(&self.pool)
        .await
        .map_err(|error| database_error("set PIN", error))?;

        if result.rows_affected() == 0 {
            return Err(ApiError::not_found(
                "IDENTITY_NOT_FOUND",
                "identity does not exist",
            ));
        }

        Ok(())
    }

    pub(super) async fn verify_pin(
        &self,
        identity_id: Uuid,
        pin: &str,
        now: u64,
    ) -> Result<PinVerificationResponse, ApiError> {
        validate_pin_format(pin)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| database_error("begin PIN verification", error))?;

        let state = sqlx::query_as::<_, PinStateRow>(
            r#"
            SELECT
                pin_hash,
                pin_failed_attempts,
                pin_locked_until_epoch_seconds
            FROM sandbox_identities
            WHERE id = $1
            FOR UPDATE
            "#,
        )
        .bind(identity_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| database_error("lock PIN state", error))?
        .ok_or_else(|| ApiError::not_found("IDENTITY_NOT_FOUND", "identity does not exist"))?;

        let mut failed_attempts = u8::try_from(state.pin_failed_attempts)
            .map_err(|_| ApiError::internal("stored PIN attempt count is invalid"))?;
        let mut locked_until = state
            .pin_locked_until_epoch_seconds
            .map(from_database_epoch)
            .transpose()?;

        if let Some(lock_expiry) = locked_until {
            if now < lock_expiry {
                return Err(pin_locked_error(lock_expiry));
            }

            failed_attempts = 0;
            locked_until = None;
        }

        let pin_hash = state.pin_hash.as_deref().ok_or_else(|| {
            ApiError::conflict("PIN_NOT_CONFIGURED", "PIN has not been configured")
        })?;
        let verified = verify_protected_pin(pin, pin_hash)?;

        if verified {
            sqlx::query(
                r#"
                UPDATE sandbox_identities
                SET
                    pin_failed_attempts = 0,
                    pin_locked_until_epoch_seconds = NULL
                WHERE id = $1
                "#,
            )
            .bind(identity_id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| database_error("reset PIN state", error))?;
            transaction
                .commit()
                .await
                .map_err(|error| database_error("commit PIN verification", error))?;
            return Ok(successful_pin_verification());
        }

        failed_attempts = failed_attempts.saturating_add(1);
        let remaining_attempts = MAX_PIN_ATTEMPTS.saturating_sub(failed_attempts);

        if failed_attempts >= MAX_PIN_ATTEMPTS {
            let lock_expiry = now
                .checked_add(PIN_LOCK_SECONDS)
                .ok_or_else(|| ApiError::internal("PIN lock expiry overflow"))?;
            locked_until = Some(lock_expiry);
        }

        sqlx::query(
            r#"
            UPDATE sandbox_identities
            SET
                pin_failed_attempts = $2,
                pin_locked_until_epoch_seconds = $3
            WHERE id = $1
            "#,
        )
        .bind(identity_id)
        .bind(i16::from(failed_attempts))
        .bind(locked_until.map(to_database_epoch).transpose()?)
        .execute(&mut *transaction)
        .await
        .map_err(|error| database_error("record failed PIN attempt", error))?;
        transaction
            .commit()
            .await
            .map_err(|error| database_error("commit failed PIN attempt", error))?;

        match locked_until {
            Some(lock_expiry) => Err(pin_locked_after_failures_error(lock_expiry)),
            None => Err(incorrect_pin_error(remaining_attempts)),
        }
    }
}

impl IdentityRow {
    fn to_view(self) -> Result<IdentityView, ApiError> {
        Ok(IdentityView {
            id: self.id,
            email: self.email,
            display_name: self.display_name,
            country_code: self.country_code,
            pin_configured: self.pin_hash.is_some(),
            created_at_epoch_seconds: from_database_epoch(self.created_at_epoch_seconds)?,
        })
    }
}

fn to_database_epoch(value: u64) -> Result<i64, ApiError> {
    i64::try_from(value).map_err(|_| ApiError::internal("epoch value exceeds database range"))
}

fn from_database_epoch(value: i64) -> Result<u64, ApiError> {
    u64::try_from(value).map_err(|_| ApiError::internal("database contains a negative epoch value"))
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database_error) => database_error.code().as_deref() == Some("23505"),
        _ => false,
    }
}

fn database_error(context: &'static str, error_value: sqlx::Error) -> ApiError {
    error!(context, error = %error_value, "PostgreSQL operation failed");
    ApiError::internal("database operation failed")
}
