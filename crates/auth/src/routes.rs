use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{password, session::AccountId};

#[derive(Clone)]
pub struct AuthState {
    pub pool: db::PgPool,
    pub invite_code: String,
    pub session_ttl_secs: i64,
}

#[derive(Deserialize)]
struct SignupRequest {
    email: String,
    password: String,
    invite_code: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct MeResponse {
    id: String,
    email: String,
}

pub fn auth_router(state: AuthState) -> Router {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .with_state(state)
}

async fn signup(
    State(state): State<AuthState>,
    Json(req): Json<SignupRequest>,
) -> impl IntoResponse {
    if req.invite_code != state.invite_code {
        return StatusCode::FORBIDDEN.into_response();
    }

    let hash = match password::hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let now = unix_now();
    let account = db::NewAccount {
        id: Uuid::new_v4().to_string(),
        email: req.email,
        password_hash: hash,
        created_at: now,
    };

    match db::create_account(&state.pool, &account).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                (StatusCode::BAD_REQUEST, "email already registered").into_response()
            } else {
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

async fn login(
    State(state): State<AuthState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let account = match db::get_account_by_email(&state.pool, &req.email).await {
        Ok(Some(a)) => a,
        Ok(None) => return (jar, StatusCode::UNAUTHORIZED).into_response(),
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    match password::verify_password(&req.password, &account.password_hash) {
        Ok(true) => {}
        _ => return (jar, StatusCode::UNAUTHORIZED).into_response(),
    }

    let now = unix_now();
    let session = db::NewSession {
        id: Uuid::new_v4().to_string(),
        account_id: account.id,
        created_at: now,
        expires_at: now + state.session_ttl_secs,
    };

    match db::create_session(&state.pool, &session).await {
        Ok(_) => {}
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    }

    let cookie = Cookie::build(("session_id", session.id))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build();

    (jar.add(cookie), StatusCode::OK).into_response()
}

async fn logout(State(state): State<AuthState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get("session_id") {
        let id = cookie.value().to_string();
        let _ = db::delete_session(&state.pool, &id).await;
    }

    let removal = Cookie::build(("session_id", "")).path("/").build();
    (jar.remove(removal), StatusCode::NO_CONTENT).into_response()
}

async fn me(
    State(state): State<AuthState>,
    account_id: AccountId,
) -> impl IntoResponse {
    match db::get_account(&state.pool, &account_id.0).await {
        Ok(Some(acc)) => Json(MeResponse { id: acc.id, email: acc.email }).into_response(),
        Ok(None) => StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

// exposed for tests only
#[doc(hidden)]
pub fn __test_hash(pw: &str) -> anyhow::Result<String> { password::hash_password(pw) }
#[doc(hidden)]
pub fn __test_verify(pw: &str, hash: &str) -> anyhow::Result<bool> { password::verify_password(pw, hash) }
