# Foundation 1A — identidad sandbox

## Propósito

Este corte agrega identidad, sesiones, PIN, Pay Safe y Pay Limits sin incorporar wallet, saldo, ledger o pagos.

La implementación es funcional, pero exclusivamente de sandbox. La persistencia es en memoria y se pierde al reiniciar la API.

## Controles implementados

- Identidad con UUID estable durante la vida del proceso.
- Correo normalizado y único.
- Sesión opaca con 256 bits aleatorios.
- En el servidor solo se conserva SHA-256 del token, no el token original.
- Sesiones con una hora de vigencia.
- Autenticación Bearer para rutas protegidas.
- PIN de cuatro dígitos.
- Rechazo de patrones de PIN previsibles.
- Hash Argon2 con salt aleatorio.
- Cinco intentos fallidos antes de un bloqueo de cinco minutos.
- Revocación inmediata mediante logout.
- Límites expresados como cadenas de unidades menores enteras.
- Pagos y transferencias permanecen deshabilitados.

## Endpoints

```text
POST   /v1/sandbox/identities
POST   /v1/sandbox/sessions
GET    /v1/me
PUT    /v1/me/pin
POST   /v1/me/pin/verify
GET    /v1/me/limits
DELETE /v1/me/session
```

## Modelo de amenaza cubierto

Este corte reduce:

- exposición accidental de PIN en memoria persistente;
- filtración de tokens almacenados en el servidor;
- sesiones indefinidas;
- fuerza bruta básica contra el PIN;
- reutilización de una sesión después de logout;
- uso de rutas protegidas sin credenciales.

## Límites deliberados

No debe interpretarse como autenticación de producción. Aún faltan:

- PostgreSQL y migraciones;
- OAuth real con Apple y Google;
- prueba de posesión de correo o teléfono;
- rotación de sesiones;
- detección de dispositivo;
- biometría nativa;
- recuperación de cuenta;
- KYC/AML;
- rate limiting distribuido;
- auditoría persistente;
- gestión de secretos;
- despliegue endurecido.

## Restricciones financieras

```text
environment = sandbox
real_money_enabled = false
external_providers_enabled = false
payments_enabled = false
transfers_enabled = false
```

No existen endpoints de dinero en Foundation 1A.
