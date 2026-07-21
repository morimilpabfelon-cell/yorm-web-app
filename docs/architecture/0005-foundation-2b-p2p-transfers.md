# ADR 0005 — transferencias P2P sandbox atómicas

## Estado

Implementado en PR #10; pendiente de validación automática completa y comprobación local en Windows.

## Contexto

Foundation 2A incorporó una wallet sandbox persistente y un ledger de doble entrada. Foundation 2B permite mover valor sandbox entre dos wallets internas sin activar dinero real ni integraciones externas.

El riesgo principal es el doble gasto concurrente: dos solicitudes podrían leer el mismo saldo del emisor y confirmar ambas si no se serializan correctamente. También deben evitarse escrituras parciales, replays duplicados, autoenvíos y movimientos entre monedas distintas.

## Decisión

Se añade el endpoint:

```text
POST /v1/sandbox/transfers
```

El emisor se obtiene exclusivamente de la sesión Bearer. El cuerpo solo identifica al destinatario y el monto:

```json
{
  "recipient_identity_id": "uuid",
  "amount_minor_units": "1250"
}
```

La operación exige `Idempotency-Key` y un PIN Pay Safe previamente configurado.

## Modelo contable

Una transferencia de 12.50 PEN se registra como:

```text
Débito   wallet del emisor      1250 PEN
Crédito  wallet del receptor    1250 PEN
```

Ambas cuentas son pasivos de la plataforma. El débito reduce el pasivo frente al emisor y el crédito aumenta el pasivo frente al receptor. El total de pasivos no cambia.

## Atomicidad y bloqueo

La transferencia se ejecuta dentro de una única transacción PostgreSQL:

1. validar autoenvío, monto e idempotencia;
2. ordenar los dos `identity_id` de forma determinista;
3. bloquear ambas filas de `sandbox_wallets` con `FOR UPDATE` en ese orden;
4. validar existencia, moneda y Pay Limits;
5. calcular ambos saldos desde `ledger_entries`;
6. rechazar saldo insuficiente antes de insertar cualquier fila;
7. insertar `ledger_transactions`;
8. insertar `sandbox_p2p_transfers`;
9. insertar el débito del emisor y el crédito del receptor;
10. confirmar la transacción.

El orden determinista evita que dos transferencias opuestas bloqueen las mismas wallets en orden inverso.

## Idempotencia

La clave es global dentro de `ledger_transactions`. El fingerprint SHA-256 incluye:

```text
transaction_kind
sender_identity_id
recipient_identity_id
sender_wallet_id
recipient_wallet_id
currency
amount_minor
```

Un replay idéntico devuelve el mismo `transaction_id` y los saldos posteriores originales. Reutilizar la clave con otro destinatario, monto u operación produce `409 IDEMPOTENCY_CONFLICT`.

## Persistencia P2P

La tabla `sandbox_p2p_transfers` conserva:

```text
transaction_id
sender_wallet_id
recipient_wallet_id
amount_minor
sender_balance_after_minor
recipient_balance_after_minor
created_at_epoch_seconds
```

Los saldos posteriores se almacenan únicamente como resultado histórico de la operación para responder replays. No son una fuente mutable de saldo; el saldo vigente siempre se deriva del ledger.

## Restricciones de base de datos

La migración `0003_sandbox_p2p_transfers.sql` incorpora:

- wallets de emisor y receptor distintas;
- monto positivo;
- saldos posteriores no negativos;
- metadatos P2P inmutables;
- validación diferida al `COMMIT`;
- exactamente un débito del emisor y un crédito del receptor;
- moneda coincidente entre transacción y wallets;
- saldos posteriores coherentes con el ledger.

## Errores de dominio

```text
SELF_TRANSFER_NOT_ALLOWED
WALLET_NOT_FOUND
RECIPIENT_WALLET_NOT_FOUND
CURRENCY_MISMATCH
PAY_LIMIT_EXCEEDED
INSUFFICIENT_FUNDS
IDEMPOTENCY_KEY_REQUIRED
IDEMPOTENCY_KEY_INVALID
IDEMPOTENCY_CONFLICT
PIN_REQUIRED
```

## Pruebas requeridas

- transferencia correcta con dos asientos balanceados;
- replay idempotente sin duplicación;
- conflicto al cambiar el cuerpo;
- autoenvío rechazado;
- moneda distinta rechazada;
- saldo insuficiente sin filas parciales;
- metadatos inmutables;
- persistencia después de recrear la aplicación;
- dos transferencias concurrentes sobre el mismo saldo: una confirma y una falla.

## Consecuencias

Foundation 2B habilita únicamente movimiento interno sandbox. Permanecen fuera de alcance dinero real, bancos, comercios, tarjetas, conversión, comisiones, Pay Activity y Pay Receipt.
