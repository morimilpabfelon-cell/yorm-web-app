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
