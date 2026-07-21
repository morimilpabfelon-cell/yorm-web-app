# Foundation 0

## Propósito

Foundation 0 establece límites y herramientas compartidas antes de implementar wallet, ledger o pagos.

## Decisiones

- Monorepo con pnpm para TypeScript y Cargo para Rust.
- API sandbox en Rust/Axum.
- PostgreSQL como base de datos objetivo.
- Contratos compartidos en `@yorm/contracts`.
- Tokens visuales compartidos en `@yorm/design-tokens`.
- Mobile, web, admin y worker se mantienen como fronteras explícitas.
- No existe dinero real ni proveedor externo activo.

## Capas futuras

```text
Interfaces: mobile, web, admin
Application: casos de uso y autorización
Domain: wallet, ledger, payments, limits y receipts
Infrastructure: PostgreSQL, proveedores, eventos y observabilidad
```

## Secuencia después de Foundation 0

1. Mobile shell y sistema visual desde Figma.
2. Identidad, sesión, PIN y Pay Safe básico.
3. Wallet sandbox y ledger de doble entrada.
4. Pagos internos, Pay Activity y Pay Receipt.
5. Pay Link, Pay QR y Pay Code.
6. Comercios, checkout y conciliación.
7. Conversión y Pay Exchange Link bajo gate separado.
8. Tarjetas y proveedores regulados bajo gate separado.

## Invariantes financieras futuras

- montos en unidades menores enteras;
- moneda explícita;
- débitos iguales a créditos;
- ninguna mutación directa del saldo;
- idempotencia en operaciones que muevan valor;
- comprobante solo después de estado confirmado;
- auditoría inmutable;
- ninguna integración real sin revisión legal y de seguridad.
