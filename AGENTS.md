# Yorm Pay — instrucciones para agentes

## Fuente de verdad

1. Diseño original del fundador en Figma.
2. Issue activo y criterios de aceptación.
3. Este archivo y documentación versionada.

## Reglas obligatorias

- Construir desde cero; no copiar el repositorio anterior.
- Una fase y un pull request estrecho por vez.
- No activar dinero real, proveedores externos ni producción.
- No modificar wallet, ledger, saldos, idempotencia, settlement o reconciliación sin issue R3 separado.
- No presentar datos simulados como reales.
- No generar comprobantes de éxito antes de una confirmación backend verificable.
- Todo monto futuro debe representarse en unidades menores enteras y con moneda explícita.
- Toda operación financiera futura debe ser atómica, idempotente y auditable.
- Cambios mobile nativos requieren issue y revisión separada.
- La ejecución normal usa PostgreSQL; el backend en memoria existe solo para pruebas unitarias rápidas.
- No registrar PIN, tokens Bearer, hashes Argon2, digests de sesión, claves idempotentes ni `DATABASE_URL` en logs.
- Wallet y ledger solo pueden operar en sandbox dentro del Issue #7.
- Transacciones y asientos posteados son inmutables y todo saldo se deriva del ledger.

## Nomenclatura de producto

```text
Yorm Pay
Compliance Layer
Pay Limits
Pay Convert
Pay Exchange Link
Pay QR
Pay Code
Pay Link
Pay Merchant
Pay Touch
Pay Card
Pay Disposable Card
Pay Checkout
Pay Payouts
Pay Gateway
Pay Receipt
Pay Activity
Pay Guide
Pay Safe
Pay Card Liquidity
```

`IonExchange` es un nombre propio externo. No se usa `Ion` como prefijo de módulos de Yorm Pay.

## Paleta oficial

```text
Paper  #F6F4F1
Stone  #E4DED2
Coral  #F95C4B
Black  #000000
```

## Gate actual

```text
Issue #7
Foundation 2A
Wallet sandbox + ledger contable de doble entrada
Riesgo R3.1
Sandbox only
Sin dinero real, P2P, comercios, tarjetas ni conversión
```
