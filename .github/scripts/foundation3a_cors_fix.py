from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    content = target.read_text(encoding="utf-8")
    if content.count(old) != 1:
        raise RuntimeError(f"expected exactly one match in {path}: {old!r}")
    target.write_text(content.replace(old, new, 1), encoding="utf-8")


def write(path: str, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content.strip() + "\n", encoding="utf-8")


replace_once(
    "Cargo.toml",
    'tower-http = { version = "0.6", features = ["trace"] }',
    'tower-http = { version = "0.6", features = ["cors", "trace"] }',
)

replace_once("apps/api/src/lib.rs", "mod error;", "mod cors;\nmod error;")
replace_once(
    "apps/api/src/lib.rs",
    """pub fn app() -> Router {
    app_with_store(SandboxStore::default())
}

pub async fn app_with_database(database_url: &str) -> Result<Router, sqlx::Error> {
    Ok(app_with_store(
        SandboxStore::connect_postgres(database_url).await?,
    ))
}

fn app_with_store(store: SandboxStore) -> Router {""",
    """pub fn app() -> Router {
    app_with_store(
        SandboxStore::default(),
        cors::default_sandbox_cors_layer(),
    )
}

pub async fn app_with_database(database_url: &str) -> Result<Router, sqlx::Error> {
    let cors_layer = cors::sandbox_cors_layer_from_env()
        .unwrap_or_else(|error| panic!(\"invalid YORM_CORS_ORIGINS: {error}\"));

    Ok(app_with_store(
        SandboxStore::connect_postgres(database_url).await?,
        cors_layer,
    ))
}

fn app_with_store(store: SandboxStore, cors_layer: tower_http::cors::CorsLayer) -> Router {""",
)
replace_once(
    "apps/api/src/lib.rs",
    """        .route(\"/v1/me/session\", delete(delete_session))
        .layer(TraceLayer::new_for_http())
        .with_state(state)""",
    """        .route(\"/v1/me/session\", delete(delete_session))
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer)
        .with_state(state)""",
)

write(
    "apps/api/src/cors.rs",
    r'''
use std::{collections::BTreeSet, env, str::FromStr, time::Duration};

use axum::http::{
    HeaderName, HeaderValue, Method, Uri,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
};
use tower_http::cors::{AllowOrigin, CorsLayer};

const CORS_ENV: &str = "YORM_CORS_ORIGINS";
const DEFAULT_SANDBOX_ORIGINS: [&str; 4] = [
    "http://localhost:8081",
    "http://127.0.0.1:8081",
    "http://localhost:19006",
    "http://127.0.0.1:19006",
];

pub(crate) fn default_sandbox_cors_layer() -> CorsLayer {
    build_sandbox_cors_layer(DEFAULT_SANDBOX_ORIGINS.iter().copied())
        .expect("default sandbox CORS origins must be valid")
}

pub(crate) fn sandbox_cors_layer_from_env() -> Result<CorsLayer, String> {
    match env::var(CORS_ENV) {
        Ok(raw) => {
            let origins = raw.split(',').map(str::trim).filter(|value| !value.is_empty());
            build_sandbox_cors_layer(origins)
        }
        Err(env::VarError::NotPresent) => Ok(default_sandbox_cors_layer()),
        Err(env::VarError::NotUnicode(_)) => {
            Err(format!("{CORS_ENV} must contain valid Unicode text"))
        }
    }
}

fn build_sandbox_cors_layer<'a>(
    origins: impl IntoIterator<Item = &'a str>,
) -> Result<CorsLayer, String> {
    let mut unique_origins = BTreeSet::new();

    for raw_origin in origins {
        let origin = raw_origin.trim().trim_end_matches('/');
        validate_origin(origin)?;
        unique_origins.insert(origin.to_owned());
    }

    if unique_origins.is_empty() {
        return Err(format!(
            "{CORS_ENV} must contain at least one exact http or https origin"
        ));
    }

    let values = unique_origins
        .into_iter()
        .map(|origin| {
            HeaderValue::from_str(&origin)
                .map_err(|_| format!("invalid CORS origin header value: {origin}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(values))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            ACCEPT,
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("idempotency-key"),
        ])
        .max_age(Duration::from_secs(600)))
}

fn validate_origin(origin: &str) -> Result<(), String> {
    if origin == "*" {
        return Err("wildcard CORS origins are forbidden in Yorm Pay".to_owned());
    }

    let uri = Uri::from_str(origin).map_err(|_| format!("invalid CORS origin: {origin}"))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(format!("CORS origin must use http or https: {origin}"));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| format!("CORS origin must include a host: {origin}"))?;
    if authority.as_str().contains('@') {
        return Err(format!("CORS origin cannot contain credentials: {origin}"));
    }
    if uri.path() != "/" || uri.query().is_some() {
        return Err(format!(
            "CORS origin must not contain a path or query: {origin}"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_sandbox_cors_layer;

    #[test]
    fn rejects_wildcard_origins() {
        let error = build_sandbox_cors_layer(["*"]).expect_err("wildcard must be rejected");
        assert!(error.contains("wildcard"));
    }

    #[test]
    fn rejects_origins_with_paths() {
        let error = build_sandbox_cors_layer(["http://localhost:8081/app"])
            .expect_err("origin paths must be rejected");
        assert!(error.contains("path or query"));
    }
}
''',
)

write(
    "apps/api/tests/cors.rs",
    r'''
use axum::{
    body::Body,
    http::{
        Method, Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
            ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD, ORIGIN,
        },
    },
};
use tower::ServiceExt;

#[tokio::test]
async fn allowed_local_origin_receives_preflight_headers() {
    let response = yorm_api::app()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/v1/sandbox/identities")
                .header(ORIGIN, "http://localhost:8081")
                .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())
                .expect("preflight request should be valid"),
        )
        .await
        .expect("router should answer preflight");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("allowed origin header should be present"),
        "http://localhost:8081"
    );
    assert!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_METHODS)
            .expect("allowed methods should be present")
            .to_str()
            .expect("allowed methods should be visible text")
            .contains("POST")
    );
}

#[tokio::test]
async fn disallowed_origin_does_not_receive_cors_permission() {
    let response = yorm_api::app()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/health")
                .header(ORIGIN, "https://untrusted.example")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should answer request");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}
''',
)

replace_once(
    "apps/mobile/src/api/client.ts",
    """      throw new YormApiError(0, 'network_error', 'No fue posible conectar con la API sandbox.');""",
    """      throw new YormApiError(
        0,
        'network_error',
        `No fue posible conectar con la API sandbox en ${baseUrl}. Verifica que la API esté activa y que el origen web esté permitido por CORS.`,
      );""",
)

replace_once(
    "apps/mobile/src/api/client.test.ts",
    """  });
});
""",
    """  });

  it('reports the public API URL on network failures without leaking the token', async () => {
    const token = 'another-opaque-secret';
    const baseUrl = 'http://127.0.0.1:8787';
    const fetchImpl: typeof fetch = async () => {
      throw new TypeError('Failed to fetch');
    };
    const client = createYormApiClient({ baseUrl, fetchImpl, timeoutMs: 100 });

    await expect(client.getMe(token)).rejects.toMatchObject({
      status: 0,
      code: 'network_error',
      message: expect.stringContaining(baseUrl),
    });

    try {
      await client.getMe(token);
    } catch (error) {
      expect(String(error)).not.toContain(token);
    }
  });
});
""",
)

replace_once(
    ".github/workflows/ci.yml",
    """      - name: Typecheck
        run: pnpm typecheck
      - name: Build
        run: pnpm build""",
    """      - name: Typecheck
        run: pnpm typecheck
      - name: Test
        run: pnpm test
      - name: Build
        run: pnpm build""",
)

readme = (ROOT / "README.md").read_text(encoding="utf-8")
mobile_heading = "## Aplicación móvil sandbox"
if mobile_heading not in readme:
    raise RuntimeError("mobile README section not found")
readme = readme.split(mobile_heading, 1)[0].rstrip() + "\n\n" + r'''## Aplicación móvil sandbox

Foundation 3A incorpora un cliente Expo/React Native en `apps/mobile`.

```powershell
Copy-Item .\apps\mobile\.env.example .\apps\mobile\.env
pnpm --filter @yorm/mobile start
```

La URL pública del backend se configura con `EXPO_PUBLIC_YORM_API_URL`; nunca debe contener secretos.

### Web local

La API habilita CORS únicamente para orígenes sandbox exactos. De forma predeterminada permite:

```text
http://localhost:8081
http://127.0.0.1:8081
http://localhost:19006
http://127.0.0.1:19006
```

Para otro puerto local, configura una lista explícita y separada por comas antes de iniciar la API:

```powershell
$env:YORM_CORS_ORIGINS = "http://localhost:8082,http://127.0.0.1:8082"
```

No se permite `*`. Los preflight `OPTIONS`, `Authorization`, `Content-Type` e `Idempotency-Key` están limitados a esa lista.

### Android

- Android Emulator usa normalmente `EXPO_PUBLIC_YORM_API_URL=http://10.0.2.2:8787`.
- Un teléfono físico necesita un development build compatible y la IP LAN de la computadora.
- Durante la transición de Expo SDK 57, este proyecto no se valida escaneando el QR con Expo Go en un teléfono físico. Se utiliza web, Android Emulator o un development build.

### Validación

```powershell
pnpm typecheck
pnpm test
pnpm build
```

El cliente crea identidad, sesión y wallet únicamente en sandbox; después consulta perfil, Pay Limits, saldo, Pay Activity y Pay Receipt. El ledger sigue siendo la única fuente de verdad financiera.
'''
(ROOT / "README.md").write_text(readme, encoding="utf-8")

adr = ROOT / "docs/architecture/0007-foundation-3a-mobile-shell.md"
adr_text = adr.read_text(encoding="utf-8")
adr_text = adr_text.replace(
    "- exportación web estática;\n- validación manual en Windows contra la API PostgreSQL sandbox;",
    "- exportación web estática y flujo web contra la API con CORS sandbox;\n- validación manual en Windows contra la API PostgreSQL sandbox;",
)
adr_text += r'''

## Corrección de conectividad local

La validación inicial identificó dos límites distintos:

1. los navegadores bloqueaban las llamadas porque la API no respondía preflight CORS;
2. Expo SDK 57 no se valida mediante Expo Go en un teléfono físico durante el periodo de transición documentado por Expo.

La API incorpora una política CORS exclusiva del sandbox:

- orígenes exactos configurables mediante `YORM_CORS_ORIGINS`;
- lista local predeterminada para los puertos web de Expo;
- métodos `GET`, `POST`, `PUT`, `DELETE` y `OPTIONS`;
- cabeceras `Accept`, `Authorization`, `Content-Type` e `Idempotency-Key`;
- rechazo explícito de wildcard, credenciales, rutas y consultas dentro del origen;
- ninguna cookie ni credencial CORS habilitada.

Foundation 3A se valida en web local y Android Emulator. Un teléfono físico requiere un development build posterior o local, no Expo Go con este SDK.
'''
adr.write_text(adr_text.strip() + "\n", encoding="utf-8")
