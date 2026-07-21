# Yorm Pay

Repositorio oficial para construir desde cero el software real de **Yorm Pay**.

## Estado

```text
FOUNDATION 1B — IN PROGRESS
SANDBOX ONLY
POSTGRESQL IDENTITY PERSISTENCE
REAL MONEY DISABLED
```

La fuente de verdad visual y funcional es el diseño original del fundador en Figma. El repositorio anterior no se copia; solo puede consultarse como referencia técnica.

## Arquitectura actual

```text
apps/
  api/       API sandbox Rust/Axum + SQLx/PostgreSQL
  mobile/    frontera futura React Native/Expo
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
GET http://127.0.0.1:8787/health
GET http://127.0.0.1:8787/health/database
GET http://127.0.0.1:8787/v1/system/status
```

Validación de persistencia en Windows:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\test-postgres-identity-persistence.ps1
```

## Persistencia permitida en Foundation 1B

```text
sandbox_identities
sandbox_sessions
PIN Argon2
contador y bloqueo de PIN
digest SHA-256 de sesión
expiración y revocación
```

## Seguridad

- Sin dinero real.
- Sin proveedores externos activos.
- Sin KYC/AML en vivo.
- Sin wallet, saldos o ledger.
- Sin transferencias ni pagos.
- Sin tokens Bearer ni PIN en texto plano dentro de PostgreSQL.
- Sin afirmaciones de producción.

Tracks #5.
