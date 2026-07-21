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
- Foundation 1A usa almacenamiento en memoria; no presentar identidades o sesiones como persistentes.
- No registrar PIN, tokens Bearer ni hashes de sesión en logs.

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
Issue #3
Foundation 1A
Identidad sandbox + sesiones + PIN + Pay Safe + Pay Limits
Riesgo R2.5
Sandbox only
Sin wallet, ledger, saldo ni pagos
```
