use axum::{Extension, Router, body::Body, http::{Request, StatusCode, header}};
use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

use auth::routes::AuthState;

// ── helpers ───────────────────────────────────────────────────────────────────

async fn setup() -> (ContainerAsync<Postgres>, db::PgPool) {
    let container = Postgres::default().start().await.expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!("postgres://postgres:postgres@localhost:{port}/postgres");
    let pool = db::connect(&url).await.expect("connect");
    db::migrate(&pool).await.expect("migrate");
    (container, pool)
}

fn test_app(pool: db::PgPool) -> Router {
    let state = AuthState {
        pool: pool.clone(),
        invite_code: "testcode".into(),
        session_ttl_secs: 604800,
    };
    auth::auth_router(state).layer(Extension(pool))
}

fn json_body(val: Value) -> Body {
    Body::from(val.to_string())
}

async fn body_str(body: Body) -> String {
    let bytes = body.collect().await.expect("collect body").to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn extract_set_cookie(response: &Response) -> Option<String> {
    response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
        .map(|s: &str| s.to_string())
}

fn session_cookie_value(set_cookie: &str) -> Option<&str> {
    set_cookie
        .split(';')
        .next()
        .and_then(|pair| pair.strip_prefix("session_id="))
}

async fn post_json(app: Router, uri: &str, body: Value) -> Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(json_body(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn get_authed(app: Router, uri: &str, cookie: &str) -> Response {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn signup(pool: db::PgPool, email: &str) {
    post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email": email, "password": "password123", "invite_code": "testcode"}),
    )
    .await;
}

async fn login(pool: db::PgPool, email: &str) -> Response {
    post_json(
        test_app(pool),
        "/auth/login",
        json!({"email": email, "password": "password123"}),
    )
    .await
}

// ── password unit tests ───────────────────────────────────────────────────────

#[test]
fn test_hash_and_verify_roundtrip() {
    let hash = auth::routes::__test_hash("correct-horse").expect("hash");
    assert!(auth::routes::__test_verify("correct-horse", &hash).expect("verify"));
}

#[test]
fn test_wrong_password_rejected() {
    let hash = auth::routes::__test_hash("correct-horse").expect("hash");
    assert!(!auth::routes::__test_verify("wrong-password", &hash).expect("verify"));
}

// ── signup ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_signup_success() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"a@b.com","password":"pw","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_signup_wrong_invite_code() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"a@b.com","password":"pw","invite_code":"nope"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_signup_duplicate_email() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "dup@b.com").await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"dup@b.com","password":"pw","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── login ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_login_success_sets_cookie() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "u@b.com").await;

    let resp = login(pool, "u@b.com").await;
    assert_eq!(resp.status(), StatusCode::OK);

    let set_cookie = extract_set_cookie(&resp).expect("should set session_id cookie");
    assert!(set_cookie.contains("session_id="));
    assert!(set_cookie.to_lowercase().contains("httponly"));
}

#[tokio::test]
async fn test_login_wrong_password() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "u@b.com").await;

    let resp = post_json(
        test_app(pool),
        "/auth/login",
        json!({"email":"u@b.com","password":"wrong"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_login_unknown_email() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/login",
        json!({"email":"ghost@b.com","password":"pw"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── me ────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_me_unauthenticated() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(Request::builder().uri("/auth/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_me_returns_account_info() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "me@b.com").await;

    let login_resp = login(pool.clone(), "me@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = get_authed(test_app(pool), "/auth/me", &cookie_header).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["email"], "me@b.com");
    assert!(body["id"].is_string());
}

// ── logout ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_logout_invalidates_session() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "lo@b.com").await;

    let login_resp = login(pool.clone(), "lo@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let logout_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout_resp.status(), StatusCode::NO_CONTENT);

    let me_resp = get_authed(test_app(pool), "/auth/me", &cookie_header).await;
    assert_eq!(me_resp.status(), StatusCode::UNAUTHORIZED);
}
