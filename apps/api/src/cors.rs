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
            let origins = raw
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty());
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
