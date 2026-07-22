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
- Todo monto debe representarse en unidades menores enteras y con moneda explícita.
- Toda operación financiera debe ser atómica, idempotente y auditable.
- Cambios mobile nativos requieren issue y revisión separada.
- La ejecución normal usa PostgreSQL; el backend en memoria existe solo para pruebas unitarias rápidas.
- No registrar PIN, tokens Bearer, hashes Argon2, digests de sesión, claves idempotentes ni `DATABASE_URL` en logs.
- Wallet, ledger y P2P solo operan en sandbox.
- Transacciones, metadatos P2P y asientos posteados son inmutables; todo saldo se deriva del ledger.
- El emisor de una transferencia se deriva exclusivamente de la sesión autenticada.
- Las wallets participantes se bloquean en orden determinista antes de consultar o gastar saldo.
- Una transferencia no puede dejar saldo negativo ni escrituras parciales.
- Transferencias entre monedas distintas, autoenvíos, comercios, bancos, tarjetas y conversión permanecen deshabilitados.
- Pay Activity y Pay Receipt son proyecciones de solo lectura; no pueden crear ni modificar movimientos.
- Una identidad solo puede consultar actividad y recibos de su propia wallet.
- La paginación de actividad debe ser estable por timestamp e identificador de transacción.
- Un recibo solo puede emitirse para una transacción posteada, visible y balanceada.
- No exponer claves idempotentes, fingerprints internos ni códigos de cuenta en respuestas de actividad o recibos.

- El cliente móvil no calcula saldos ni fabrica estados financieros; consume respuestas confirmadas de la API.
- En Android/iOS el token Bearer solo puede persistirse mediante SecureStore; no usar AsyncStorage.
- Variables `EXPO_PUBLIC_*` son públicas y nunca pueden contener secretos.
- La exportación web no persiste la sesión entre recargas.
- Foundation 3A no incluye envío P2P, crédito sandbox, biometría, notificaciones, cámara, QR, NFC ni publicación en tiendas.

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
Issue #13
Foundation 3A
Base móvil Expo/React Native y cliente API sandbox
Riesgo R4.1
Sandbox only
Sin dinero real, bancos, comercios, tarjetas ni conversión
```
