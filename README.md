# Yorm Pay

Repositorio oficial para construir desde cero el software real de **Yorm Pay**.

## Estado

```text
FOUNDATION 3A — IN PROGRESS
SANDBOX ONLY
REAL MONEY DISABLED
```

La fuente de verdad visual y funcional es el diseño original del fundador en Figma. El repositorio anterior no se copia; solo puede consultarse como referencia técnica.

## Arquitectura actual

```text
apps/
  api/       API sandbox Rust/Axum + SQLx/PostgreSQL
  mobile/    Expo/React Native — cliente sandbox
  web/       frontera futura Next.js
  admin/     frontera futura de operaciones
  worker/    frontera futura de tareas y conciliación
packages/
  contracts/      contratos TypeScript
  design-tokens/  paleta y tokens visuales
infra/
  docker/    PostgreSQL local
```

## Requisitos

- Node.js 24
- pnpm 10.34.5
- Rust stable
- Docker Desktop con Docker Compose

## Preparar PostgreSQL local

```powershell
cd C:\Users\morim\yorm-web-app

docker compose -f .\infra\docker\compose.yml up -d postgres

$env:DATABASE_URL = "postgres://yorm:yorm_local_only@127.0.0.1:5432/yorm_pay?sslmode=disable"
$env:YORM_API_ADDR = "127.0.0.1:8787"
```

Las migraciones de `apps/api/migrations` se aplican automáticamente al iniciar la API.

## Validación

```powershell
corepack enable
pnpm install --frozen-lockfile
pnpm typecheck
pnpm build
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace
cargo run -p yorm-api
```

API local:

```text
GET    http://127.0.0.1:8787/health
GET    http://127.0.0.1:8787/health/database
GET    http://127.0.0.1:8787/v1/system/status
POST   http://127.0.0.1:8787/v1/me/wallet
GET    http://127.0.0.1:8787/v1/me/wallet
POST   http://127.0.0.1:8787/v1/sandbox/wallet/credits
POST   http://127.0.0.1:8787/v1/sandbox/transfers
GET    http://127.0.0.1:8787/v1/me/activity
GET    http://127.0.0.1:8787/v1/me/receipts/{transaction_id}
DELETE http://127.0.0.1:8787/v1/me/session
```

Validación integral de Foundation 2C en Windows:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\test-activity-receipts-sandbox.ps1
```

## Persistencia sandbox

```text
sandbox_identities
sandbox_sessions
PIN Argon2
contador y bloqueo de PIN
digest SHA-256 de sesión
sandbox_wallets
ledger_accounts
ledger_transactions
ledger_entries
sandbox_p2p_transfers
```

Pay Activity y Pay Receipt no crean tablas adicionales: se derivan del ledger confirmado.

## Invariantes financieras

- Todos los montos usan unidades menores enteras; nunca `float`.
- Los saldos se derivan de asientos y no tienen columna mutable.
- Cada transacción confirmada mantiene débitos iguales a créditos.
- Los asientos, transacciones y metadatos P2P son inmutables.
- Los créditos y transferencias exigen `Idempotency-Key`.
- Las transferencias bloquean las dos wallets en orden determinista.
- Saldo insuficiente no crea transacciones ni asientos parciales.
- Autoenvíos y transferencias entre monedas distintas se rechazan.
- Pay Activity y Pay Receipt son proyecciones de solo lectura.
- Una identidad solo puede consultar operaciones de su propia wallet.
- Los recibos se generan únicamente para transacciones posteadas y balanceadas.

## Seguridad

- Sin dinero real.
- Sin proveedores externos activos.
- Sin KYC/AML en vivo.
- Sin bancos, depósitos o retiros externos.
- Sin pagos a comercios.
- Sin tarjetas ni conversión de divisas.
- Sin claves idempotentes, fingerprints internos ni códigos de cuenta en Activity o Receipt.
- Sin tokens Bearer ni PIN en texto plano dentro de PostgreSQL.
- Sin afirmaciones de producción.

Tracks #13.

## Aplicación móvil sandbox

Foundation 3A incorpora un cliente Expo/React Native en `apps/mobile`.

```powershell
Copy-Item .\apps\mobile\.env.example .\apps\mobile\.env
pnpm --filter @yorm/mobile start
```

La URL pública del backend se configura con `EXPO_PUBLIC_YORM_API_URL`; nunca debe contener secretos.

### Web local

La API habilita CORS únicamente para orígenes sandbox exactos. De forma predeterminada permite:

```text
http://localhost:8081
http://127.0.0.1:8081
http://localhost:19006
http://127.0.0.1:19006
```

Para otro puerto local, configura una lista explícita y separada por comas antes de iniciar la API:

```powershell
$env:YORM_CORS_ORIGINS = "http://localhost:8082,http://127.0.0.1:8082"
```

No se permite `*`. Los preflight `OPTIONS`, `Authorization`, `Content-Type` e `Idempotency-Key` están limitados a esa lista.

### Android

- Android Emulator usa normalmente `EXPO_PUBLIC_YORM_API_URL=http://10.0.2.2:8787`.
- Un teléfono físico necesita un development build compatible y la IP LAN de la computadora.
- Durante la transición de Expo SDK 57, este proyecto no se valida escaneando el QR con Expo Go en un teléfono físico. Se utiliza web, Android Emulator o un development build.

### Validación

```powershell
pnpm typecheck
pnpm test
pnpm build
```

El cliente crea identidad, sesión y wallet únicamente en sandbox; después consulta perfil, Pay Limits, saldo, Pay Activity y Pay Receipt. El ledger sigue siendo la única fuente de verdad financiera.
