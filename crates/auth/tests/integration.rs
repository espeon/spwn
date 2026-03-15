use axum::response::Response;
use axum::{
    Extension, Router,
    body::Body,
    http::{Request, StatusCode, header},
};
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
        public_url: "http://localhost:3019".into(),
        gateway_secret: None,
        ssh_gateway_addr: "localhost:2222".into(),
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
    signup_with_username(
        pool,
        email,
        &email.split('@').next().unwrap_or("user").replace('.', "-"),
    )
    .await;
}

async fn signup_with_username(pool: db::PgPool, email: &str, username: &str) {
    post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email": email, "password": "password123", "username": username, "invite_code": "testcode"}),
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

#[test]
fn test_verify_invalid_hash_returns_err() {
    assert!(auth::routes::__test_verify("password", "not-a-valid-argon2-hash").is_err());
}

// ── signup ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_signup_success() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"a@b.com","password":"pw","username":"alice","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_signup_creates_personal_namespace() {
    let (_c, pool) = setup().await;
    post_json(
        test_app(pool.clone()),
        "/auth/signup",
        json!({"email":"ns@example.com","password":"pw","username":"ns-alice","invite_code":"testcode"}),
    )
    .await;

    // The account should exist and have a personal namespace.
    let acct = db::get_account_by_email(&pool, "ns@example.com")
        .await
        .expect("query")
        .expect("account should exist");

    let ns = db::get_personal_namespace(&pool, &acct.id)
        .await
        .expect("query")
        .expect("personal namespace should be created on signup");

    assert_eq!(ns.kind, "personal");
    assert_eq!(ns.owner_id, acct.id);
    assert_eq!(ns.slug, "ns-alice");
    assert_eq!(ns.vm_limit, 5);

    // The owner should be a member with role 'owner'.
    let membership = db::get_namespace_member(&pool, &ns.id, &acct.id)
        .await
        .expect("query")
        .expect("owner membership should exist");
    assert_eq!(membership.role, "owner");
}

#[tokio::test]
async fn test_signup_wrong_invite_code() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"a@b.com","password":"pw","username":"alice","invite_code":"nope"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_signup_duplicate_username() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "first@b.com", "taken").await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"second@b.com","password":"pw","username":"taken","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_signup_invalid_username_too_short() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"a@b.com","password":"pw","username":"ab","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_signup_invalid_username_special_chars() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"a@b.com","password":"pw","username":"bad_name!","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_signup_duplicate_email() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "dup@b.com", "dupuser").await;
    let resp = post_json(
        test_app(pool),
        "/auth/signup",
        json!({"email":"dup@b.com","password":"pw","username":"dupuser2","invite_code":"testcode"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── login ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_login_success_sets_cookie() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "user@b.com").await;

    let resp = login(pool, "user@b.com").await;
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
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_me_returns_account_info() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "meuser@b.com").await;

    let login_resp = login(pool.clone(), "meuser@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = get_authed(test_app(pool), "/auth/me", &cookie_header).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["email"], "meuser@b.com");
    assert!(body["id"].is_string());
    assert!(body["username"].is_string());
    assert_eq!(body["has_avatar"], false);
}

// ── logout ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_logout_invalidates_session() {
    let (_c, pool) = setup().await;
    signup(pool.clone(), "logout@b.com").await;

    let login_resp = login(pool.clone(), "logout@b.com").await;
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

// ── profile update ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_update_profile_display_name() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "prof@b.com", "profuser").await;

    let login_resp = login(pool.clone(), "prof@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/auth/me")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie_header)
                .body(Body::from(json!({"display_name": "Prof User"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let me_resp = get_authed(test_app(pool), "/auth/me", &cookie_header).await;
    let body: Value = serde_json::from_str(&body_str(me_resp.into_body()).await).unwrap();
    assert_eq!(body["display_name"], "Prof User");
}

#[tokio::test]
async fn test_update_profile_unauthenticated() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/auth/me")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"display_name": "Nope"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── server config ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_server_config() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["ssh_gateway_addr"], "localhost:2222");
}

// ── theme ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_update_theme() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "theme@b.com", "themeuser").await;

    let login_resp = login(pool.clone(), "theme@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/auth/me/theme")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie_header)
                .body(Body::from(json!({"theme": "dracula"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let me_resp = get_authed(test_app(pool), "/auth/me", &cookie_header).await;
    let body: Value = serde_json::from_str(&body_str(me_resp.into_body()).await).unwrap();
    assert_eq!(body["theme"], "dracula");
}

// ── avatar ────────────────────────────────────────────────────────────────────

fn minimal_png() -> Vec<u8> {
    let img = image::RgbImage::new(4, 4);
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

async fn upload_avatar(app: axum::Router, cookie_header: &str, content_type: &str, body: Vec<u8>) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/auth/me/avatar")
            .header(header::CONTENT_TYPE, content_type)
            .header(header::COOKIE, cookie_header)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn test_upload_avatar_wrong_content_type() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "avbad@b.com", "avbaduser").await;

    let login_resp = login(pool.clone(), "avbad@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = upload_avatar(test_app(pool), &cookie_header, "application/json", vec![1, 2, 3]).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_upload_avatar_invalid_image_data() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "avgarbage@b.com", "avgarbage").await;

    let login_resp = login(pool.clone(), "avgarbage@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = upload_avatar(test_app(pool), &cookie_header, "image/png", vec![0xFF, 0x00, 0xAB]).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_upload_and_fetch_avatar() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "avok@b.com", "avokuser").await;

    let login_resp = login(pool.clone(), "avok@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = upload_avatar(test_app(pool.clone()), &cookie_header, "image/png", minimal_png()).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let me_resp = get_authed(test_app(pool.clone()), "/auth/me", &cookie_header).await;
    let body: Value = serde_json::from_str(&body_str(me_resp.into_body()).await).unwrap();
    let account_id = body["id"].as_str().unwrap().to_string();

    let avatar_resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri(format!("/auth/avatar/{account_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(avatar_resp.status(), StatusCode::OK);
    assert_eq!(
        avatar_resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
}

#[tokio::test]
async fn test_avatar_not_found_for_unknown_account() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri("/auth/avatar/nonexistent-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── SSH keys ──────────────────────────────────────────────────────────────────

const TEST_PUBKEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GkZH test@spwn";

#[tokio::test]
async fn test_ssh_keys_list_empty() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "keys@b.com", "keysuser").await;

    let login_resp = login(pool.clone(), "keys@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = get_authed(test_app(pool), "/api/account/keys", &cookie_header).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn test_ssh_key_add_and_delete() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "addkey@b.com", "addkeyuser").await;

    let login_resp = login(pool.clone(), "addkey@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account/keys")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie_header)
                .body(Body::from(
                    json!({"name": "my-laptop", "public_key": TEST_PUBKEY}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    let key_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["name"], "my-laptop");
    assert!(body["fingerprint"].as_str().unwrap().starts_with("SHA256:"));

    let list_resp = get_authed(test_app(pool.clone()), "/api/account/keys", &cookie_header).await;
    let list: Vec<Value> = serde_json::from_str(&body_str(list_resp.into_body()).await).unwrap();
    assert_eq!(list.len(), 1);

    let del_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/account/keys/{key_id}"))
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

    let list_resp = get_authed(test_app(pool), "/api/account/keys", &cookie_header).await;
    let list: Vec<Value> = serde_json::from_str(&body_str(list_resp.into_body()).await).unwrap();
    assert_eq!(list.len(), 0);
}

#[tokio::test]
async fn test_ssh_key_add_invalid_key() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "badkey@b.com", "badkeyuser").await;

    let login_resp = login(pool.clone(), "badkey@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account/keys")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie_header)
                .body(Body::from(
                    json!({"name": "bad", "public_key": "not-a-valid-key"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ssh_key_delete_not_found() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "delkey@b.com", "delkeyuser").await;

    let login_resp = login(pool.clone(), "delkey@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/account/keys/nonexistent-key-id")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── CLI auth flow ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_cli_init_creates_code() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/cli/init")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert!(body["code"].as_str().unwrap().len() > 0);
    assert!(body["browser_url"].as_str().unwrap().contains("cli-auth?code="));
    assert_eq!(body["expires_in"], 300);
}

#[tokio::test]
async fn test_cli_poll_pending_then_authorized() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "clipoll@b.com", "clipolluser").await;

    let login_resp = login(pool.clone(), "clipoll@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let init_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/cli/init")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let init_body: Value = serde_json::from_str(&body_str(init_resp.into_body()).await).unwrap();
    let code = init_body["code"].as_str().unwrap().to_string();

    let poll_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/auth/cli/poll?code={code}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let poll_body: Value = serde_json::from_str(&body_str(poll_resp.into_body()).await).unwrap();
    assert_eq!(poll_body["status"], "pending");
    assert!(poll_body["token"].is_null());

    let auth_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/cli/authorize")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie_header)
                .body(Body::from(json!({"code": code}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth_resp.status(), StatusCode::NO_CONTENT);

    let poll_resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri(format!("/auth/cli/poll?code={code}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let poll_body: Value = serde_json::from_str(&body_str(poll_resp.into_body()).await).unwrap();
    assert_eq!(poll_body["status"], "authorized");
    assert!(poll_body["token"].as_str().unwrap().starts_with("spwn_tok_"));
}

#[tokio::test]
async fn test_cli_poll_missing_code_returns_expired() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri("/auth/cli/poll?code=does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["status"], "expired");
}

#[tokio::test]
async fn test_cli_deny() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "clideny@b.com", "clidenyuser").await;

    let login_resp = login(pool.clone(), "clideny@b.com").await;
    let set_cookie = extract_set_cookie(&login_resp).expect("cookie");
    let session_id = session_cookie_value(&set_cookie).expect("session_id");
    let cookie_header = format!("session_id={session_id}");

    let init_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/cli/init")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let init_body: Value = serde_json::from_str(&body_str(init_resp.into_body()).await).unwrap();
    let code = init_body["code"].as_str().unwrap().to_string();

    let deny_resp = test_app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/cli/deny")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie_header)
                .body(Body::from(json!({"code": code}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deny_resp.status(), StatusCode::NO_CONTENT);

    let poll_resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri(format!("/auth/cli/poll?code={code}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let poll_body: Value = serde_json::from_str(&body_str(poll_resp.into_body()).await).unwrap();
    assert_eq!(poll_body["status"], "denied");
}

// ── gateway endpoints ─────────────────────────────────────────────────────────

fn test_app_with_gateway(pool: db::PgPool) -> axum::Router {
    use axum::Extension;
    let state = auth::routes::AuthState {
        pool: pool.clone(),
        invite_code: "testcode".into(),
        session_ttl_secs: 604800,
        public_url: "http://localhost:3019".into(),
        gateway_secret: Some("gw-secret".into()),
        ssh_gateway_addr: "localhost:2222".into(),
    };
    auth::auth_router(state).layer(Extension(pool))
}

#[tokio::test]
async fn test_gateway_auth_password_no_secret_returns_401() {
    let (_c, pool) = setup().await;
    let resp = post_json(
        test_app_with_gateway(pool),
        "/internal/gateway/auth/password",
        json!({"username": "user@b.com", "password": "pw"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_gateway_auth_password_valid_credentials() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "gwauth@b.com", "gwauthuser").await;

    let resp = test_app_with_gateway(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/gateway/auth/password")
                .header(header::CONTENT_TYPE, "application/json")
                .header("authorization", "Bearer gw-secret")
                .body(Body::from(
                    json!({"username": "gwauth@b.com", "password": "password123"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["ok"], true);
    assert_eq!(body["username"], "gwauthuser");
}

#[tokio::test]
async fn test_gateway_auth_password_wrong_password() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "gwbad@b.com", "gwbaduser").await;

    let resp = test_app_with_gateway(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/gateway/auth/password")
                .header(header::CONTENT_TYPE, "application/json")
                .header("authorization", "Bearer gw-secret")
                .body(Body::from(
                    json!({"username": "gwbad@b.com", "password": "wrong"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["ok"], false);
}

#[tokio::test]
async fn test_gateway_auth_pubkey_unknown_key() {
    let (_c, pool) = setup().await;
    let resp = test_app_with_gateway(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/gateway/auth/pubkey")
                .header(header::CONTENT_TYPE, "application/json")
                .header("authorization", "Bearer gw-secret")
                .body(Body::from(
                    json!({"fingerprint": "SHA256:notarealkey"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["ok"], false);
}

#[tokio::test]
async fn test_gateway_lookup_vm_missing_params() {
    let (_c, pool) = setup().await;
    let resp = test_app_with_gateway(pool)
        .oneshot(
            Request::builder()
                .uri("/internal/gateway/vm")
                .header("authorization", "Bearer gw-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_gateway_lookup_vm_not_found() {
    let (_c, pool) = setup().await;
    let resp = test_app_with_gateway(pool)
        .oneshot(
            Request::builder()
                .uri("/internal/gateway/vm?vm_id=nonexistent")
                .header("authorization", "Bearer gw-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Bearer token auth (session.rs coverage) ──────────────────────────────────

async fn create_api_token(pool: &db::PgPool, account_id: &str, raw_token: &str) {
    use sha2::{Digest, Sha256};
    let token_hash = hex::encode(Sha256::digest(raw_token.as_bytes()));
    db::create_api_token(
        pool,
        &db::NewApiToken {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account_id.to_string(),
            token_hash,
            name: "test-token".into(),
        },
    )
    .await
    .expect("create api token");
}

#[tokio::test]
async fn test_bearer_token_auth_me_returns_200() {
    let (_c, pool) = setup().await;
    signup_with_username(pool.clone(), "bearer@b.com", "beareruser").await;

    let account = db::get_account_by_email(&pool, "bearer@b.com")
        .await
        .unwrap()
        .unwrap();
    create_api_token(&pool, &account.id, "my-raw-token-abc123").await;

    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .header(header::AUTHORIZATION, "Bearer my-raw-token-abc123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_str(&body_str(resp.into_body()).await).unwrap();
    assert_eq!(body["username"], "beareruser");
}

#[tokio::test]
async fn test_bearer_token_invalid_returns_401() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .header(header::AUTHORIZATION, "Bearer totally-not-a-valid-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_bearer_token_malformed_returns_401() {
    let (_c, pool) = setup().await;
    let resp = test_app(pool)
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .header(header::AUTHORIZATION, "NotBearer something")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
