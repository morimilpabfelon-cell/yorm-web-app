# Yorm Pay

Repositorio oficial para construir desde cero el software real de **Yorm Pay**.

## Estado

```text
FOUNDATION 0 — IN PROGRESS
SANDBOX ONLY
REAL MONEY DISABLED
```

La fuente de verdad visual y funcional es el diseño original del fundador en Figma. El repositorio anterior no se copia; solo puede consultarse como referencia técnica.

## Arquitectura inicial

```text
apps/
  api/       API sandbox Rust/Axum
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
- Docker Desktop, opcional para PostgreSQL local

## Validación

```powershell
corepack enable
pnpm install
pnpm typecheck
pnpm build
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo run -p yorm-api
```

API local:

```text
GET http://127.0.0.1:8787/health
GET http://127.0.0.1:8787/v1/system/status
```

## Seguridad

- Sin dinero real.
- Sin proveedores externos activos.
- Sin KYC/AML en vivo.
- Sin ledger productivo.
- Sin afirmaciones de producción.

Tracks #1.
