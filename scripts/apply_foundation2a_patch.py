from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing expected block: {label}")
    return text.replace(old, new, 1)


store_path = Path("apps/api/src/store.rs")
store = store_path.read_text()
store = replace_once(store, "mod postgres;", "mod ledger;\nmod postgres;", "store module declarations")
store = replace_once(
    store,
    "    model::{IdentityView, PayLimitsResponse, PinVerificationResponse, SessionResponse},",
    "    model::{\n        IdentityView, PayLimitsResponse, PinVerificationResponse, SandboxCreditResponse,\n        SessionResponse, WalletView,\n    },",
    "store model imports",
)
store = replace_once(
    store,
    "use self::postgres::PostgresStore;",
    "use self::{ledger::LedgerStore, postgres::PostgresStore};",
    "store backend imports",
)
store = replace_once(
    store,
    "    Postgres(PostgresStore),",
    "    Postgres {\n        identity: PostgresStore,\n        ledger: LedgerStore,\n    },",
    "store postgres variant",
)
store = replace_once(
    store,
    '''    pub async fn connect_postgres(database_url: &str) -> Result<Self, sqlx::Error> {
        Ok(Self {
            backend: StoreBackend::Postgres(PostgresStore::connect(database_url).await?),
        })
    }
''',
    '''    pub async fn connect_postgres(database_url: &str) -> Result<Self, sqlx::Error> {
        let identity = PostgresStore::connect(database_url).await?;
        let ledger = LedgerStore::new(identity.pool());
        Ok(Self {
            backend: StoreBackend::Postgres { identity, ledger },
        })
    }
''',
    "connect postgres",
)
store = replace_once(
    store,
    '            StoreBackend::Postgres(_) => "postgres",',
    '            StoreBackend::Postgres { .. } => "postgres",',
    "backend name",
)
store = store.replace(
    "StoreBackend::Postgres(store)",
    "StoreBackend::Postgres { identity: store, .. }",
)
wallet_methods = '''    pub async fn create_wallet(
        &self,
        identity_id: Uuid,
        now: u64,
    ) -> Result<WalletView, ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Err(ApiError::service_unavailable(
                "DATABASE_REQUIRED",
                "wallet operations require the PostgreSQL sandbox backend",
            )),
            StoreBackend::Postgres { ledger, .. } => ledger.create_wallet(identity_id, now).await,
        }
    }

    pub async fn get_wallet(&self, identity_id: Uuid) -> Result<WalletView, ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Err(ApiError::service_unavailable(
                "DATABASE_REQUIRED",
                "wallet operations require the PostgreSQL sandbox backend",
            )),
            StoreBackend::Postgres { ledger, .. } => ledger.get_wallet(identity_id).await,
        }
    }

    pub async fn credit_wallet(
        &self,
        identity_id: Uuid,
        idempotency_key: &str,
        amount_minor_units: &str,
        now: u64,
    ) -> Result<SandboxCreditResponse, ApiError> {
        match &self.backend {
            StoreBackend::Memory(_) => Err(ApiError::service_unavailable(
                "DATABASE_REQUIRED",
                "wallet operations require the PostgreSQL sandbox backend",
            )),
            StoreBackend::Postgres { ledger, .. } => {
                ledger
                    .credit_wallet(identity_id, idempotency_key, amount_minor_units, now)
                    .await
            }
        }
    }

'''
store = replace_once(
    store,
    "    pub fn limits_for_country(country_code: &str) -> PayLimitsResponse {",
    wallet_methods + "    pub fn limits_for_country(country_code: &str) -> PayLimitsResponse {",
    "wallet store methods",
)
store_path.write_text(store)

postgres_path = Path("apps/api/src/store/postgres.rs")
postgres = postgres_path.read_text()
postgres = replace_once(
    postgres,
    "impl PostgresStore {\n    pub(super) async fn connect",
    "impl PostgresStore {\n    pub(super) fn pool(&self) -> PgPool {\n        self.pool.clone()\n    }\n\n    pub(super) async fn connect",
    "postgres pool accessor",
)
postgres_path.write_text(postgres)

error_path = Path("apps/api/src/error.rs")
error_text = error_path.read_text()
error_text = replace_once(
    error_text,
    '''    pub fn locked(code: &'static str, message: impl Into<String>) -> Self {
        let status = StatusCode::from_u16(423).expect("423 is a valid HTTP status code");
        Self::new(status, code, message)
    }
''',
    '''    pub fn locked(code: &'static str, message: impl Into<String>) -> Self {
        let status = StatusCode::from_u16(423).expect("423 is a valid HTTP status code");
        Self::new(status, code, message)
    }

    pub fn service_unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }
''',
    "service unavailable error",
)
error_path.write_text(error_text)

lib_path = Path("apps/api/src/lib.rs")
lib = lib_path.read_text()
lib = replace_once(
    lib,
    '''        CreateIdentityRequest, CreateSessionRequest, IdentityView, PayLimitsResponse, PinRequest,
        PinVerificationResponse, SessionResponse,
''',
    '''        CreateIdentityRequest, CreateSessionRequest, IdentityView, PayLimitsResponse, PinRequest,
        PinVerificationResponse, SandboxCreditRequest, SandboxCreditResponse, SessionResponse,
        WalletView,
''',
    "lib model imports",
)
lib = replace_once(
    lib,
    '''        .route("/v1/me/limits", get(get_limits))
        .route("/v1/me/session", delete(delete_session))
''',
    '''        .route("/v1/me/limits", get(get_limits))
        .route("/v1/me/wallet", post(create_wallet).get(get_wallet))
        .route("/v1/sandbox/wallet/credits", post(credit_wallet))
        .route("/v1/me/session", delete(delete_session))
''',
    "wallet routes",
)
wallet_handlers = '''async fn create_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<WalletView>), ApiError> {
    let identity = authenticate(&headers, &state).await?;
    let wallet = state
        .store
        .create_wallet(identity.id, epoch_seconds())
        .await?;
    Ok((StatusCode::CREATED, Json(wallet)))
}

async fn get_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletView>, ApiError> {
    let identity = authenticate(&headers, &state).await?;
    Ok(Json(state.store.get_wallet(identity.id).await?))
}

async fn credit_wallet(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SandboxCreditRequest>,
) -> Result<(StatusCode, Json<SandboxCreditResponse>), ApiError> {
    let identity = authenticate(&headers, &state).await?;
    if !identity.view.pin_configured {
        return Err(ApiError::conflict(
            "PIN_REQUIRED",
            "configure Pay Safe PIN before using sandbox wallet credits",
        ));
    }
    let key = idempotency_key(&headers)?;
    let credit = state
        .store
        .credit_wallet(
            identity.id,
            key,
            &request.amount_minor_units,
            epoch_seconds(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(credit)))
}

'''
lib = replace_once(
    lib,
    "async fn delete_session(\n",
    wallet_handlers + "async fn delete_session(\n",
    "wallet handlers",
)
idempotency_helper = '''fn idempotency_key(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("idempotency-key")
        .ok_or_else(|| {
            ApiError::bad_request(
                "IDEMPOTENCY_KEY_REQUIRED",
                "Idempotency-Key header is required",
            )
        })?
        .to_str()
        .map_err(|_| {
            ApiError::bad_request(
                "IDEMPOTENCY_KEY_INVALID",
                "Idempotency-Key header must contain visible text",
            )
        })
}

'''
lib = replace_once(
    lib,
    "fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {",
    idempotency_helper + "fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {",
    "idempotency helper",
)
lib_path.write_text(lib)

contracts_path = Path("packages/contracts/src/index.ts")
contracts = contracts_path.read_text()
wallet_contracts = '''export interface WalletView {
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

'''
contracts = replace_once(
    contracts,
    "export interface ApiErrorResponse {",
    wallet_contracts + "export interface ApiErrorResponse {",
    "wallet TypeScript contracts",
)
contracts_path.write_text(contracts)

agents_path = Path("AGENTS.md")
agents = agents_path.read_text()
agents = agents.replace(
    "- Foundation 1A usa almacenamiento en memoria; no presentar identidades o sesiones como persistentes.",
    "- La ejecución normal usa PostgreSQL; el backend en memoria existe solo para pruebas unitarias rápidas.",
)
gate_marker = "## Gate actual\n"
if gate_marker not in agents:
    raise SystemExit("missing AGENTS gate marker")
agents = agents.split(gate_marker, 1)[0] + gate_marker + '''
```text
Issue #7
Foundation 2A
Wallet sandbox + ledger contable de doble entrada
Riesgo R3.1
Sandbox only
Sin dinero real, P2P, comercios, tarjetas ni conversión
```
'''
agents_path.write_text(agents)

readme_path = Path("README.md")
readme = readme_path.read_text()
readme = replace_once(
    readme,
    '''FOUNDATION 0 — IN PROGRESS
SANDBOX ONLY
REAL MONEY DISABLED''',
    '''FOUNDATION 2A — IN PROGRESS
SANDBOX ONLY
REAL MONEY DISABLED''',
    "README status",
)
readme = replace_once(
    readme,
    '''GET http://127.0.0.1:8787/health
GET http://127.0.0.1:8787/v1/system/status''',
    '''GET  http://127.0.0.1:8787/health
GET  http://127.0.0.1:8787/health/database
GET  http://127.0.0.1:8787/v1/system/status
POST http://127.0.0.1:8787/v1/me/wallet
GET  http://127.0.0.1:8787/v1/me/wallet
POST http://127.0.0.1:8787/v1/sandbox/wallet/credits''',
    "README endpoints",
)
readme = readme.replace("Tracks #1.", "Tracks #7.")
readme_path.write_text(readme)
