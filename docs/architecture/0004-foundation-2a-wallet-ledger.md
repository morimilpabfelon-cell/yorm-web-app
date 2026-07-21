# ADR 0004 — wallet sandbox y ledger de doble entrada

## Estado

Implementado en PR #8; pendiente de validación automática completa y comprobación local en Windows.

## Contexto

Foundation 1B incorporó persistencia PostgreSQL para identidad, sesiones y PIN. Foundation 2A abre el primer dominio financiero de Yorm Pay, pero permanece estrictamente en `sandbox` y no mueve dinero real.

El objetivo es validar las invariantes contables antes de añadir transferencias P2P, pagos, tarjetas o conversión.

## Decisión

Cada identidad puede crear una wallet en su moneda local. La wallet se vincula a una cuenta contable de pasivo con lado normal crédito.

El fondeo sandbox utiliza una cuenta sistémica de activo con lado normal débito.

Ejemplo de crédito de 1.250 unidades menores PEN:

```text
Débito   sandbox_funding:PEN       1250
Crédito  wallet:<identity>:PEN     1250
```

El saldo visible de la wallet se calcula en cada consulta:

```text
SUM(créditos) - SUM(débitos)
```

No existe una columna mutable de saldo en `sandbox_wallets`.

## Tablas

```text
ledger_accounts
sandbox_wallets
ledger_transactions
ledger_entries
```

### `ledger_accounts`

Representa cuentas sistémicas y cuentas de wallet. Cada cuenta tiene moneda, clase contable y lado normal.

```text
asset      -> debit
liability  -> credit
```

### `sandbox_wallets`

Relaciona una identidad con su cuenta contable y moneda. Una identidad solo puede tener una wallet por moneda.

### `ledger_transactions`

Registra operaciones posteadas e idempotentes. Foundation 2A permite únicamente `sandbox_credit` desde la API.

La tabla conserva:

```text
transaction_kind
currency
idempotency_key
request_fingerprint
resulting_balance_minor
posted_at_epoch_seconds
```

`resulting_balance_minor` es una instantánea inmutable del resultado de esa transacción; no es el saldo operativo de la wallet.

### `ledger_entries`

Cada asiento contiene:

```text
transaction_id
account_id
entry_side
amount_minor
created_at_epoch_seconds
```

`amount_minor` es `BIGINT`, estrictamente positivo y nunca representa decimales o flotantes.

## Balance diferido

PostgreSQL ejecuta triggers de restricción diferidos al confirmar la transacción SQL.

Para cada `ledger_transaction` se exige:

- al menos dos asientos;
- suma de débitos igual a suma de créditos;
- misma moneda entre transacción y cuentas afectadas.

Una transacción desbalanceada puede insertar temporalmente filas dentro de una transacción SQL, pero el `COMMIT` falla.

## Inmutabilidad

Triggers `BEFORE UPDATE OR DELETE` rechazan modificaciones sobre:

```text
ledger_accounts
sandbox_wallets
ledger_transactions
ledger_entries
```

Las correcciones futuras deben realizarse mediante nuevas transacciones compensatorias, nunca reescribiendo historia.

## Idempotencia

`POST /v1/sandbox/wallet/credits` requiere `Idempotency-Key`.

La huella SHA-256 incluye:

```text
kind
identity_id
wallet_id
currency
amount_minor
```

Repetir la misma clave y solicitud devuelve la transacción existente. Reutilizar la clave con otro monto o wallet devuelve `409 IDEMPOTENCY_CONFLICT`.

## Concurrencia

La wallet se bloquea mediante `FOR UPDATE` durante el crédito sandbox. Esto serializa el cálculo de saldo anterior, la instantánea resultante y la publicación de asientos.

## Pay Limits

El crédito sandbox no puede superar el límite por operación correspondiente al país de la identidad.

Este control no habilita pagos ni transferencias; ambos siguen desactivados.

## Endpoints

```text
POST /v1/me/wallet
GET  /v1/me/wallet
POST /v1/sandbox/wallet/credits
```

El crédito requiere sesión válida, PIN configurado y clave idempotente.

## Fuera de alcance

- transferencias P2P;
- pagos a comercios;
- depósitos o retiros reales;
- tarjetas;
- conversión de divisas;
- comisiones;
- settlement o reconciliación externa;
- KYC/AML real;
- producción.

## Validación

CI levanta PostgreSQL 17 y comprueba:

- formato y Clippy sin warnings;
- pruebas unitarias;
- migraciones;
- creación y persistencia de wallet;
- crédito balanceado con dos asientos;
- idempotencia sin duplicación;
- conflicto de clave reutilizada;
- ausencia de columna mutable de saldo;
- rechazo de UPDATE/DELETE;
- rechazo de transacción desbalanceada al confirmar;
- persistencia después de recrear el router.

Windows dispone de:

```text
scripts/test-wallet-ledger-sandbox.ps1
```

## Consecuencias

- Yorm Pay dispone de un núcleo contable sandbox auditable.
- El saldo ya no puede implementarse mediante asignaciones directas.
- Foundation 2B podrá construir transferencias P2P usando débitos y créditos entre cuentas de wallet.
- La existencia del ledger sandbox no implica preparación para dinero real.
