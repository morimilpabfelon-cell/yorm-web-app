mod postgres;

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{IdentityView, PayLimitsResponse, PinVerificationResponse, SessionResponse},
};

use self::postgres::PostgresStore;

pub(super) const SESSION_TTL_SECONDS: u64 = 60 * 60;
pub(super) const MAX_PIN_ATTEMPTS: u8 = 5;
pub(super) const PIN_LOCK_SECONDS: u64 = 5 * 60;

#[derive(Clone)]
pub struct SandboxStore {
    backend: StoreBackend,
}

#[derive(Clone)]
enum StoreBackend {
    Memory(Arc<MemoryStore>),
    Postgres(PostgresStore),
}

#[derive(Default)]
struct MemoryStore {
    inner: RwLock<MemoryData>,
}

#[derive(Default)]
struct MemoryData {
    identities: HashMap<Uuid, IdentityRecord>,
    identity_by_email: HashMap<String, Uuid>,
    sessions: HashMap<String, SessionRecord>,
}

#[derive(Clone)]
struct IdentityRecord {
    id: Uuid,
    email: String,
    display_name: String,
    country_code: String,
    pin_hash: Option<String>,
    pin_failed_attempts: u8,
    pin_locked_until_epoch_seconds: Option<u64>,
    created_at_epoch_seconds: u64,
}

#[derive(Clone)]
struct SessionRecord {
    identity_id: Uuid,
    expires_at_epoch_seconds: u64,
    revoked_at_epoch_seconds: Option<u64>,
}

pub struct AuthenticatedIdentity {
    pub id: Uuid,
    pub view: IdentityView,
}

impl Default for SandboxStore {
    fn default() -> Self {
        Self {
            backend: StoreBackend::Memory(Arc::new(MemoryStore::default())),
        }
    }
}

impl SandboxStore {
    pub async fn connect_postgres(database_url: &str) -> Result<Self, sqlx::Error> {
        Ok(Self {
            backend: StoreBackend::Postgres(PostgresStore::connect(database_url).await?),
        })
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            StoreBackend::Memory(_) => "memory",
            StoreBackend::Postgres(_) => "postgres",
        }
    }

    pub async fn database_health(&self) -> Result<(), ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Ok(()),
            StoreBackend::Postgres(store) => store.health().await,
        }
    }

    pub async fn register_identity(
        &self,
        email: &str,
        display_name: &str,
        country_code: &str,
        now: u64,
    ) -> Result<IdentityView, ApiError> {
        match &self.backend {
            StoreBackend::Memory(store) => {
                store.register_identity(email, display_name, country_code, now)
            }
            StoreBackend::Postgres(store) => {
                store
                    .register_identity(email, display_name, country_code, now)
                    .await
            }
        }
    }

    pub async fn create_session(
        &self,
        identity_id: Uuid,
        now: u64,
    ) -> Result<SessionResponse, ApiError> {
        match &self.backend {
            StoreBackend::Memory(store) => store.create_session(identity_id, now),
            StoreBackend::Postgres(store) => store.create_session(identity_id, now).await,
        }
    }

    pub async fn authenticate(
        &self,
        access_token: &str,
        now: u64,
    ) -> Result<AuthenticatedIdentity, ApiError> {
        match &self.backend {
            StoreBackend::Memory(store) => store.authenticate(access_token, now),
            StoreBackend::Postgres(store) => store.authenticate(access_token, now).await,
        }
    }

    pub async fn revoke_session(&self, access_token: &str, now: u64) -> Result<(), ApiError> {
        match &self.backend {
            StoreBackend::Memory(store) => store.revoke_session(access_token, now),
            StoreBackend::Postgres(store) => store.revoke_session(access_token, now).await,
        }
    }

    pub async fn set_pin(&self, identity_id: Uuid, pin: &str) -> Result<(), ApiError> {
        match &self.backend {
            StoreBackend::Memory(store) => store.set_pin(identity_id, pin),
            StoreBackend::Postgres(store) => store.set_pin(identity_id, pin).await,
        }
    }

    pub async fn verify_pin(
        &self,
        identity_id: Uuid,
        pin: &str,
        now: u64,
    ) -> Result<PinVerificationResponse, ApiError> {
        match &self.backend {
            StoreBackend::Memory(store) => store.verify_pin(identity_id, pin, now),
            StoreBackend::Postgres(store) => store.verify_pin(identity_id, pin, now).await,
        }
    }

    pub fn limits_for_country(country_code: &str) -> PayLimitsResponse {
        let (currency, per_operation, daily, monthly) = match country_code {
            "PE" => ("PEN", 100_000_u64, 300_000_u64, 1_500_000_u64),
            "BR" => ("BRL", 100_000_u64, 300_000_u64, 1_500_000_u64),
            "MX" => ("MXN", 500_000_u64, 1_500_000_u64, 7_500_000_u64),
            "CO" => (
                "COP",
                100_000_000_u64,
                300_000_000_u64,
                1_500_000_000_u64,
            ),
            _ => ("USD", 25_000_u64, 75_000_u64, 375_000_u64),
        };

        PayLimitsResponse {
            module: "Pay Limits",
            environment: "sandbox",
            currency: currency.to_owned(),
            per_operation_minor_units: per_operation.to_string(),
            daily_minor_units: daily.to_string(),
            monthly_minor_units: monthly.to_string(),
            payments_enabled: false,
            transfers_enabled: false,
            kyc_tier: "sandbox_unverified",
        }
    }
}

impl MemoryStore {
    fn register_identity(
        &self,
        email: &str,
        display_name: &str,
        country_code: &str,
        now: u64,
    ) -> Result<IdentityView, ApiError> {
        let email = normalize_email(email)?;
        let display_name = normalize_display_name(display_name)?;
        let country_code = normalize_country_code(country_code)?;

        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("identity store lock poisoned"))?;

        if data.identity_by_email.contains_key(&email) {
            return Err(ApiError::conflict(
                "IDENTITY_ALREADY_EXISTS",
                "an identity with this email already exists",
            ));
        }

        let record = IdentityRecord {
            id: Uuid::new_v4(),
            email: email.clone(),
            display_name,
            country_code,
            pin_hash: None,
            pin_failed_attempts: 0,
            pin_locked_until_epoch_seconds: None,
            created_at_epoch_seconds: now,
        };
        let view = record.to_view();

        data.identity_by_email.insert(email, record.id);
        data.identities.insert(record.id, record);

        Ok(view)
    }

    fn create_session(&self, identity_id: Uuid, now: u64) -> Result<SessionResponse, ApiError> {
        let (access_token, token_digest) = generate_session_token();
        let expires_at_epoch_seconds = now + SESSION_TTL_SECONDS;

        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("session store lock poisoned"))?;

        if !data.identities.contains_key(&identity_id) {
            return Err(ApiError::not_found(
                "IDENTITY_NOT_FOUND",
                "identity does not exist",
            ));
        }

        data.sessions.insert(
            token_digest,
            SessionRecord {
                identity_id,
                expires_at_epoch_seconds,
                revoked_at_epoch_seconds: None,
            },
        );

        Ok(SessionResponse {
            access_token,
            token_type: "Bearer",
            expires_at_epoch_seconds,
        })
    }

    fn authenticate(
        &self,
        access_token: &str,
        now: u64,
    ) -> Result<AuthenticatedIdentity, ApiError> {
        let token_digest = digest_token(access_token);
        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("session store lock poisoned"))?;

        let session = data.sessions.get(&token_digest).cloned().ok_or_else(|| {
            ApiError::unauthorized("SESSION_INVALID", "session is invalid or revoked")
        })?;

        if session.revoked_at_epoch_seconds.is_some() {
            return Err(ApiError::unauthorized(
                "SESSION_INVALID",
                "session is invalid or revoked",
            ));
        }

        if session.expires_at_epoch_seconds <= now {
            return Err(ApiError::unauthorized(
                "SESSION_EXPIRED",
                "session has expired",
            ));
        }

        let identity = data
            .identities
            .get(&session.identity_id)
            .ok_or_else(|| ApiError::internal("session identity is missing"))?;

        Ok(AuthenticatedIdentity {
            id: identity.id,
            view: identity.to_view(),
        })
    }

    fn revoke_session(&self, access_token: &str, now: u64) -> Result<(), ApiError> {
        let token_digest = digest_token(access_token);
        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("session store lock poisoned"))?;
        let session = data.sessions.get_mut(&token_digest).ok_or_else(|| {
            ApiError::unauthorized("SESSION_INVALID", "session is invalid or revoked")
        })?;

        if session.revoked_at_epoch_seconds.is_some() {
            return Err(ApiError::unauthorized(
                "SESSION_INVALID",
                "session is invalid or revoked",
            ));
        }

        session.revoked_at_epoch_seconds = Some(now);
        Ok(())
    }

    fn set_pin(&self, identity_id: Uuid, pin: &str) -> Result<(), ApiError> {
        let pin_hash = protect_pin(pin)?;

        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("identity store lock poisoned"))?;
        let identity = data
            .identities
            .get_mut(&identity_id)
            .ok_or_else(|| ApiError::not_found("IDENTITY_NOT_FOUND", "identity does not exist"))?;

        identity.pin_hash = Some(pin_hash);
        identity.pin_failed_attempts = 0;
        identity.pin_locked_until_epoch_seconds = None;

        Ok(())
    }

    fn verify_pin(
        &self,
        identity_id: Uuid,
        pin: &str,
        now: u64,
    ) -> Result<PinVerificationResponse, ApiError> {
        validate_pin_format(pin)?;

        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("identity store lock poisoned"))?;
        let identity = data
            .identities
            .get_mut(&identity_id)
            .ok_or_else(|| ApiError::not_found("IDENTITY_NOT_FOUND", "identity does not exist"))?;

        if let Some(locked_until) = identity.pin_locked_until_epoch_seconds {
            if now < locked_until {
                return Err(pin_locked_error(locked_until));
            }

            identity.pin_failed_attempts = 0;
            identity.pin_locked_until_epoch_seconds = None;
        }

        let pin_hash = identity.pin_hash.as_deref().ok_or_else(|| {
            ApiError::conflict("PIN_NOT_CONFIGURED", "PIN has not been configured")
        })?;
        let verified = verify_protected_pin(pin, pin_hash)?;

        if verified {
            identity.pin_failed_attempts = 0;
            identity.pin_locked_until_epoch_seconds = None;
            return Ok(successful_pin_verification());
        }

        identity.pin_failed_attempts = identity.pin_failed_attempts.saturating_add(1);
        let remaining_attempts = MAX_PIN_ATTEMPTS.saturating_sub(identity.pin_failed_attempts);

        if identity.pin_failed_attempts >= MAX_PIN_ATTEMPTS {
            let locked_until = now + PIN_LOCK_SECONDS;
            identity.pin_locked_until_epoch_seconds = Some(locked_until);
            return Err(pin_locked_after_failures_error(locked_until));
        }

        Err(incorrect_pin_error(remaining_attempts))
    }
}

impl IdentityRecord {
    fn to_view(&self) -> IdentityView {
        IdentityView {
            id: self.id,
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            country_code: self.country_code.clone(),
            pin_configured: self.pin_hash.is_some(),
            created_at_epoch_seconds: self.created_at_epoch_seconds,
        }
    }
}

pub fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn generate_session_token() -> (String, String) {
    let mut token_bytes = [0_u8; 32];
    let mut rng = OsRng;
    rng.fill_bytes(&mut token_bytes);
    let access_token = URL_SAFE_NO_PAD.encode(token_bytes);
    let token_digest = digest_token(&access_token);
    (access_token, token_digest)
}

pub(super) fn digest_token(access_token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(access_token.as_bytes()))
}

pub(super) fn normalize_email(email: &str) -> Result<String, ApiError> {
    let normalized = email.trim().to_ascii_lowercase();
    let mut parts = normalized.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();

    if local.is_empty() || domain.is_empty() || parts.next().is_some() || !domain.contains('.') {
        return Err(ApiError::bad_request(
            "EMAIL_INVALID",
            "email address is invalid",
        ));
    }

    Ok(normalized)
}

pub(super) fn normalize_display_name(display_name: &str) -> Result<String, ApiError> {
    let normalized = display_name.trim();
    let character_count = normalized.chars().count();

    if !(2..=80).contains(&character_count) {
        return Err(ApiError::bad_request(
            "DISPLAY_NAME_INVALID",
            "display name must contain between 2 and 80 characters",
        ));
    }

    Ok(normalized.to_owned())
}

pub(super) fn normalize_country_code(country_code: &str) -> Result<String, ApiError> {
    let normalized = country_code.trim().to_ascii_uppercase();

    if normalized.len() != 2 || !normalized.bytes().all(|value| value.is_ascii_uppercase()) {
        return Err(ApiError::bad_request(
            "COUNTRY_CODE_INVALID",
            "country code must be an ISO 3166-1 alpha-2 code",
        ));
    }

    Ok(normalized)
}

pub(super) fn validate_pin_format(pin: &str) -> Result<(), ApiError> {
    if pin.len() != 4 || !pin.bytes().all(|value| value.is_ascii_digit()) {
        return Err(ApiError::bad_request(
            "PIN_INVALID_FORMAT",
            "PIN must contain exactly four digits",
        ));
    }

    Ok(())
}

pub(super) fn validate_new_pin(pin: &str) -> Result<(), ApiError> {
    const WEAK_PINS: [&str; 7] = ["0000", "1111", "1234", "2222", "4321", "5555", "2580"];

    validate_pin_format(pin)?;

    if WEAK_PINS.contains(&pin) {
        return Err(ApiError::bad_request(
            "PIN_TOO_WEAK",
            "choose a less predictable PIN",
        ));
    }

    Ok(())
}

pub(super) fn protect_pin(pin: &str) -> Result<String, ApiError> {
    validate_new_pin(pin)?;
    let mut rng = OsRng;
    let salt = SaltString::generate(&mut rng);
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map_err(|_| ApiError::internal("failed to protect PIN"))
        .map(|hash| hash.to_string())
}

pub(super) fn verify_protected_pin(pin: &str, pin_hash: &str) -> Result<bool, ApiError> {
    let parsed_hash = PasswordHash::new(pin_hash)
        .map_err(|_| ApiError::internal("stored PIN hash is invalid"))?;
    Ok(Argon2::default()
        .verify_password(pin.as_bytes(), &parsed_hash)
        .is_ok())
}

pub(super) fn successful_pin_verification() -> PinVerificationResponse {
    PinVerificationResponse {
        verified: true,
        remaining_attempts: MAX_PIN_ATTEMPTS,
        locked_until_epoch_seconds: None,
    }
}

pub(super) fn incorrect_pin_error(remaining_attempts: u8) -> ApiError {
    ApiError::unauthorized(
        "PIN_INCORRECT",
        format!("PIN is incorrect; {remaining_attempts} attempts remain"),
    )
}

pub(super) fn pin_locked_error(locked_until: u64) -> ApiError {
    ApiError::locked(
        "PIN_LOCKED",
        format!("PIN is locked until epoch second {locked_until}"),
    )
}

pub(super) fn pin_locked_after_failures_error(locked_until: u64) -> ApiError {
    ApiError::locked(
        "PIN_LOCKED",
        format!("too many failed attempts; PIN is locked until epoch second {locked_until}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{SandboxStore, StoreBackend};

    #[tokio::test]
    async fn pin_is_hashed_and_can_be_verified() {
        let store = SandboxStore::default();
        let identity = store
            .register_identity("alex@example.com", "Alex", "PE", 100)
            .await
            .expect("identity should be created");

        store
            .set_pin(identity.id, "4096")
            .await
            .expect("PIN should be configured");

        let StoreBackend::Memory(memory) = &store.backend else {
            panic!("unit test must use memory backend");
        };
        let data = memory.inner.read().expect("store should be readable");
        let record = data
            .identities
            .get(&identity.id)
            .expect("identity should exist");
        let stored_hash = record.pin_hash.as_deref().expect("hash should exist");
        assert_ne!(stored_hash, "4096");
        assert!(stored_hash.starts_with("$argon2"));
        drop(data);

        let response = store
            .verify_pin(identity.id, "4096", 101)
            .await
            .expect("correct PIN should verify");
        assert!(response.verified);
    }

    #[tokio::test]
    async fn five_failed_attempts_lock_the_pin() {
        let store = SandboxStore::default();
        let identity = store
            .register_identity("lock@example.com", "Lock Test", "PE", 100)
            .await
            .expect("identity should be created");
        store
            .set_pin(identity.id, "4096")
            .await
            .expect("PIN should be configured");

        for attempt in 0..5 {
            assert!(
                store
                    .verify_pin(identity.id, "9876", 200 + attempt)
                    .await
                    .is_err()
            );
        }

        let StoreBackend::Memory(memory) = &store.backend else {
            panic!("unit test must use memory backend");
        };
        let data = memory.inner.read().expect("store should be readable");
        let record = data
            .identities
            .get(&identity.id)
            .expect("identity should exist");
        assert_eq!(record.pin_failed_attempts, 5);
        assert_eq!(record.pin_locked_until_epoch_seconds, Some(504));
    }

    #[tokio::test]
    async fn logout_revokes_the_session() {
        let store = SandboxStore::default();
        let identity = store
            .register_identity("logout@example.com", "Logout Test", "PE", 100)
            .await
            .expect("identity should be created");
        let session = store
            .create_session(identity.id, 101)
            .await
            .expect("session should be created");

        store
            .authenticate(&session.access_token, 102)
            .await
            .expect("session should authenticate");
        store
            .revoke_session(&session.access_token, 103)
            .await
            .expect("session should be revoked");
        assert!(
            store
                .authenticate(&session.access_token, 104)
                .await
                .is_err()
        );
    }
}
