use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::SaltString,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::ApiError,
    model::{IdentityView, PayLimitsResponse, PinVerificationResponse, SessionResponse},
};

const SESSION_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_PIN_ATTEMPTS: u8 = 5;
const PIN_LOCK_DURATION: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct SandboxStore {
    inner: RwLock<StoreData>,
}

#[derive(Default)]
struct StoreData {
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
}

pub struct AuthenticatedIdentity {
    pub id: Uuid,
    pub view: IdentityView,
}

impl SandboxStore {
    pub fn register_identity(
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

    pub fn create_session(
        &self,
        identity_id: Uuid,
        now: u64,
    ) -> Result<SessionResponse, ApiError> {
        let mut token_bytes = [0_u8; 32];
        let mut rng = OsRng;
        rng.fill_bytes(&mut token_bytes);
        let access_token = URL_SAFE_NO_PAD.encode(token_bytes);
        let token_digest = digest_token(&access_token);
        let expires_at_epoch_seconds = now + SESSION_TTL.as_secs();

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
            },
        );

        Ok(SessionResponse {
            access_token,
            token_type: "Bearer",
            expires_at_epoch_seconds,
        })
    }

    pub fn authenticate(
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

        if session.expires_at_epoch_seconds <= now {
            data.sessions.remove(&token_digest);
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

    pub fn revoke_session(&self, access_token: &str) -> Result<(), ApiError> {
        let token_digest = digest_token(access_token);
        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("session store lock poisoned"))?;

        if data.sessions.remove(&token_digest).is_none() {
            return Err(ApiError::unauthorized(
                "SESSION_INVALID",
                "session is invalid or revoked",
            ));
        }

        Ok(())
    }

    pub fn set_pin(&self, identity_id: Uuid, pin: &str) -> Result<(), ApiError> {
        validate_pin(pin)?;

        let mut rng = OsRng;
        let salt = SaltString::generate(&mut rng);
        let pin_hash = Argon2::default()
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|_| ApiError::internal("failed to protect PIN"))?
            .to_string();

        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("identity store lock poisoned"))?;
        let identity = data.identities.get_mut(&identity_id).ok_or_else(|| {
            ApiError::not_found("IDENTITY_NOT_FOUND", "identity does not exist")
        })?;

        identity.pin_hash = Some(pin_hash);
        identity.pin_failed_attempts = 0;
        identity.pin_locked_until_epoch_seconds = None;

        Ok(())
    }

    pub fn verify_pin(
        &self,
        identity_id: Uuid,
        pin: &str,
        now: u64,
    ) -> Result<PinVerificationResponse, ApiError> {
        if pin.len() != 4 || !pin.bytes().all(|value| value.is_ascii_digit()) {
            return Err(ApiError::bad_request(
                "PIN_INVALID_FORMAT",
                "PIN must contain exactly four digits",
            ));
        }

        let mut data = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("identity store lock poisoned"))?;
        let identity = data.identities.get_mut(&identity_id).ok_or_else(|| {
            ApiError::not_found("IDENTITY_NOT_FOUND", "identity does not exist")
        })?;

        if let Some(locked_until) = identity.pin_locked_until_epoch_seconds {
            if now < locked_until {
                return Err(ApiError::locked(
                    "PIN_LOCKED",
                    format!("PIN is locked until epoch second {locked_until}"),
                ));
            }

            identity.pin_failed_attempts = 0;
            identity.pin_locked_until_epoch_seconds = None;
        }

        let pin_hash = identity.pin_hash.as_deref().ok_or_else(|| {
            ApiError::conflict("PIN_NOT_CONFIGURED", "PIN has not been configured")
        })?;
        let parsed_hash = PasswordHash::new(pin_hash)
            .map_err(|_| ApiError::internal("stored PIN hash is invalid"))?;
        let verified = Argon2::default()
            .verify_password(pin.as_bytes(), &parsed_hash)
            .is_ok();

        if verified {
            identity.pin_failed_attempts = 0;
            identity.pin_locked_until_epoch_seconds = None;
            return Ok(PinVerificationResponse {
                verified: true,
                remaining_attempts: MAX_PIN_ATTEMPTS,
                locked_until_epoch_seconds: None,
            });
        }

        identity.pin_failed_attempts = identity.pin_failed_attempts.saturating_add(1);
        let remaining_attempts = MAX_PIN_ATTEMPTS.saturating_sub(identity.pin_failed_attempts);

        if identity.pin_failed_attempts >= MAX_PIN_ATTEMPTS {
            let locked_until = now + PIN_LOCK_DURATION.as_secs();
            identity.pin_locked_until_epoch_seconds = Some(locked_until);
            return Err(ApiError::locked(
                "PIN_LOCKED",
                format!("too many failed attempts; PIN is locked until epoch second {locked_until}"),
            ));
        }

        Err(ApiError::unauthorized(
            "PIN_INCORRECT",
            format!("PIN is incorrect; {remaining_attempts} attempts remain"),
        ))
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

fn digest_token(access_token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(access_token.as_bytes()))
}

fn normalize_email(email: &str) -> Result<String, ApiError> {
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

fn normalize_display_name(display_name: &str) -> Result<String, ApiError> {
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

fn normalize_country_code(country_code: &str) -> Result<String, ApiError> {
    let normalized = country_code.trim().to_ascii_uppercase();

    if normalized.len() != 2 || !normalized.bytes().all(|value| value.is_ascii_uppercase()) {
        return Err(ApiError::bad_request(
            "COUNTRY_CODE_INVALID",
            "country code must be an ISO 3166-1 alpha-2 code",
        ));
    }

    Ok(normalized)
}

fn validate_pin(pin: &str) -> Result<(), ApiError> {
    const WEAK_PINS: [&str; 7] = ["0000", "1111", "1234", "2222", "4321", "5555", "2580"];

    if pin.len() != 4 || !pin.bytes().all(|value| value.is_ascii_digit()) {
        return Err(ApiError::bad_request(
            "PIN_INVALID_FORMAT",
            "PIN must contain exactly four digits",
        ));
    }

    if WEAK_PINS.contains(&pin) {
        return Err(ApiError::bad_request(
            "PIN_TOO_WEAK",
            "choose a less predictable PIN",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SandboxStore;

    #[test]
    fn pin_is_hashed_and_can_be_verified() {
        let store = SandboxStore::default();
        let identity = store
            .register_identity("alex@example.com", "Alex", "PE", 100)
            .expect("identity should be created");

        store
            .set_pin(identity.id, "4096")
            .expect("PIN should be configured");

        let data = store.inner.read().expect("store should be readable");
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
            .expect("correct PIN should verify");
        assert!(response.verified);
    }

    #[test]
    fn five_failed_attempts_lock_the_pin() {
        let store = SandboxStore::default();
        let identity = store
            .register_identity("lock@example.com", "Lock Test", "PE", 100)
            .expect("identity should be created");
        store
            .set_pin(identity.id, "4096")
            .expect("PIN should be configured");

        for attempt in 0..5 {
            assert!(store.verify_pin(identity.id, "9876", 200 + attempt).is_err());
        }

        let data = store.inner.read().expect("store should be readable");
        let record = data
            .identities
            .get(&identity.id)
            .expect("identity should exist");
        assert_eq!(record.pin_failed_attempts, 5);
        assert_eq!(record.pin_locked_until_epoch_seconds, Some(504));
    }

    #[test]
    fn logout_revokes_the_session() {
        let store = SandboxStore::default();
        let identity = store
            .register_identity("logout@example.com", "Logout Test", "PE", 100)
            .expect("identity should be created");
        let session = store
            .create_session(identity.id, 101)
            .expect("session should be created");

        store
            .authenticate(&session.access_token, 102)
            .expect("session should authenticate");
        store
            .revoke_session(&session.access_token)
            .expect("session should be revoked");
        assert!(store.authenticate(&session.access_token, 103).is_err());
    }
}
