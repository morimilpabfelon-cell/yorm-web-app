# ADR 0003 — persistencia PostgreSQL de identidad sandbox

## Estado

Propuesto en Issue #5 y PR #6.

## Contexto

Foundation 1A implementó identidad, sesiones, PIN, Pay Safe básico y Pay Limits mediante un almacén en memoria. Ese backend permitió validar contratos y seguridad básica, pero toda identidad y sesión desaparecía al reiniciar la API.

Foundation 1B incorpora persistencia sin abrir todavía el dominio financiero. La base de datos solo puede contener identidad sandbox, autenticación y estado de seguridad.

## Decisión

La ejecución normal de `yorm-api` requiere `DATABASE_URL` y utiliza PostgreSQL mediante SQLx. El backend en memoria permanece disponible exclusivamente para pruebas unitarias rápidas.

Las migraciones se encuentran en:

```text
apps/api/migrations
```

La API las ejecuta durante el arranque antes de aceptar conexiones HTTP.

## Modelo persistido

### `sandbox_identities`

```text
id
email normalizado y único
display_name
country_code
pin_hash Argon2 opcional
pin_failed_attempts
pin_locked_until_epoch_seconds
created_at_epoch_seconds
```

### `sandbox_sessions`

```text
token_digest SHA-256
identity_id
expires_at_epoch_seconds
revoked_at_epoch_seconds
created_at_epoch_seconds
```

No existe una columna para el token Bearer bruto. El token solo se entrega una vez al cliente; PostgreSQL conserva únicamente su digest.

## Consistencia y concurrencia

La verificación del PIN abre una transacción y obtiene la fila de identidad mediante `SELECT ... FOR UPDATE`. El contador de intentos, el reinicio del contador y el bloqueo temporal se escriben dentro de la misma transacción.

Esto evita que verificaciones concurrentes pierdan incrementos o permitan superar el límite de cinco intentos.

## Liveness y readiness

```text
GET /health
```

Confirma que el proceso HTTP está vivo y no consulta PostgreSQL.

```text
GET /health/database
```

Ejecuta una consulta mínima y confirma que el backend activo es `postgres`.

## Límites deliberados

Foundation 1B no incorpora:

- wallet;
- saldos;
- ledger;
- transferencias;
- pagos;
- proveedores externos;
- KYC/AML real;
- producción.

No se crearán tablas financieras dentro de este gate.

## Validación

CI levanta PostgreSQL 17 y ejecuta:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace
```

La prueba de integración recrea el router contra la misma base de datos y verifica:

- persistencia de identidad;
- persistencia de sesión;
- persistencia del PIN;
- persistencia del bloqueo;
- revocación después de reinicio;
- ausencia del token bruto en las columnas;
- hash Argon2 del PIN;
- digest de sesión.

Windows dispone del script:

```text
scripts/test-postgres-identity-persistence.ps1
```

## Consecuencias

- Docker Desktop deja de ser opcional para ejecutar la API local completa.
- El binario falla al iniciar cuando `DATABASE_URL` no existe o PostgreSQL no está disponible.
- Los datos sandbox sobreviven al reinicio de la API y del equipo mientras se conserve el volumen Docker.
- La persistencia no implica que el sistema esté preparado para dinero real.
