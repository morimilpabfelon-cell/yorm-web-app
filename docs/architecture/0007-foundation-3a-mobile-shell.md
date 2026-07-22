# ADR 0007 — Foundation 3A mobile shell

## Estado

Propuesto.

## Contexto

Foundation 2C completó el primer vertical slice backend sandbox: identidad, sesión, PIN, wallet, ledger, crédito sandbox, transferencia P2P, Pay Activity y Pay Receipt.

La siguiente frontera es una aplicación móvil construida desde cero que consuma ese backend sin duplicar reglas financieras ni presentar datos simulados como reales.

## Decisión

Foundation 3A inicializa `apps/mobile` con Expo SDK 57, React Native, Expo Router y TypeScript estricto.

El cliente móvil:

- utiliza `@yorm/design-tokens` como fuente de colores, radios y espaciado;
- obtiene la URL del backend desde `EXPO_PUBLIC_YORM_API_URL`;
- persiste el token Bearer con SecureStore en Android/iOS;
- crea identidad, sesión y wallet únicamente en sandbox;
- consulta perfil, Pay Limits, wallet, Pay Activity y Pay Receipt;
- revoca la sesión backend al cerrar sesión;
- no calcula saldos ni genera comprobantes localmente;
- muestra permanentemente el entorno `SANDBOX`.

## Límites

Este gate no incorpora envío P2P desde la UI, crédito sandbox desde la UI, KYC/AML real, biometría, notificaciones, cámara, QR, NFC, contactos, builds de tienda, bancos, comercios, tarjetas, conversión ni dinero real.

## Seguridad

- No registrar tokens, PIN, `DATABASE_URL` ni cuerpos sensibles.
- No incluir secretos en variables `EXPO_PUBLIC_*`.
- No almacenar el token en AsyncStorage.
- Invalidar el estado local incluso si la revocación remota falla por conectividad.
- Tratar toda respuesta financiera como dato backend de solo lectura.

## Validación prevista

- instalación reproducible con pnpm;
- TypeScript estricto;
- pruebas unitarias del cliente y sesión;
- exportación web estática;
- validación manual en Windows contra la API PostgreSQL sandbox;
- repositorio limpio antes de marcar el PR listo para revisión.

Tracks #13.
