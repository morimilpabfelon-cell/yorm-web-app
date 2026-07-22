# ADR 0006 — Foundation 2C: Pay Activity y Pay Receipt

## Estado

Propuesto para el Issue #11.

## Contexto

Foundation 2A estableció el ledger inmutable y Foundation 2B añadió transferencias P2P sandbox atómicas. La aplicación necesita presentar historial y comprobantes sin introducir una segunda fuente de verdad financiera.

## Decisión

Pay Activity y Pay Receipt se implementan como proyecciones autenticadas de solo lectura sobre:

- `sandbox_wallets`;
- `ledger_transactions`;
- `ledger_entries`;
- `sandbox_p2p_transfers`;
- `sandbox_identities` únicamente para la contraparte visible en sandbox.

No se crean tablas de actividad ni recibos.

### Pay Activity

```text
GET /v1/me/activity?limit=20&cursor=<opaque>
```

La lista se limita a la wallet de la identidad autenticada y se ordena de forma estable por:

```text
posted_at_epoch_seconds DESC, transaction_id DESC
```

El cursor base64url contiene exclusivamente la posición de paginación; no contiene secretos ni datos de autenticación.

### Pay Receipt

```text
GET /v1/me/receipts/{transaction_id}
```

El comprobante solo se devuelve cuando la wallet autenticada participa en la transacción. Se genera desde datos posteados e inmutables y contiene una referencia SHA-256 base64url determinista calculada sobre una representación canónica de la perspectiva de esa wallet.

## Invariantes

- lectura únicamente;
- autorización horizontal por wallet;
- montos enteros en unidades menores;
- moneda explícita;
- débitos iguales a créditos;
- saldos posteriores históricos tomados del movimiento confirmado;
- sin claves idempotentes, fingerprints internos ni códigos de cuenta en la respuesta;
- sin recibos antes de confirmación backend;
- sin dinero real ni proveedores externos.

## Consecuencias

- el ledger permanece como única fuente de verdad;
- los recibos pueden regenerarse de forma determinista;
- la paginación no cambia de orden cuando varias operaciones comparten timestamp;
- una corrupción contable provoca error interno en lugar de emitir un comprobante falso.

## Fuera de alcance

PDF, firma jurídica, descarga, notificaciones, búsqueda textual, filtros avanzados, comercios, bancos, tarjetas, conversión, KYC/AML real y producción.

Tracks #11.
