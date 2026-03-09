use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, patch, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use hex;
use image::{ImageFormat, imageops::FilterType};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{password, session::AccountId};

#[derive(Clone)]
pub struct AuthState {
    pub pool: db::PgPool,
    pub invite_code: String,
    pub session_ttl_secs: i64,
    pub public_url: String,
}

#[derive(Deserialize)]
struct SignupRequest {
    email: String,
    password: String,
    username: String,
    invite_code: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct UpdateProfileRequest {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct UpdateThemeRequest {
    theme: String,
}

#[derive(Serialize)]
struct MeResponse {
    id: String,
    email: String,
    username: String,
    display_name: Option<String>,
    has_avatar: bool,
    theme: String,
    vm_limit: i32,
    vcpu_limit: i32,
    mem_limit_mb: i32,
}

pub fn auth_router(state: AuthState) -> Router {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/me", patch(update_profile))
        .route("/auth/me/avatar", post(upload_avatar))
        .route("/auth/me/theme", patch(update_theme))
        .route("/auth/avatar/{account_id}", get(get_avatar))
        .route("/auth/cli/init", post(cli_init))
        .route("/auth/cli/poll", get(cli_poll))
        .route("/auth/cli/authorize", post(cli_authorize))
        .route("/auth/cli/deny", post(cli_deny))
        .with_state(state)
}

// ── CLI device auth ────────────────────────────────────────────────────────────

const CLI_CODE_TTL_SECS: i64 = 300;

#[derive(Deserialize)]
struct CliInitQuery {
    base_url: Option<String>,
}

#[derive(Serialize)]
struct CliInitResponse {
    code: String,
    browser_url: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct CliPollQuery {
    code: String,
}

#[derive(Serialize)]
struct CliPollResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Deserialize)]
struct CliCodeBody {
    code: String,
}

async fn cli_init(
    State(state): State<AuthState>,
    Query(query): Query<CliInitQuery>,
) -> impl IntoResponse {
    let code: String = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();
    let code = code.to_lowercase();

    let now = unix_now();
    let expires_at = now + CLI_CODE_TTL_SECS;

    if let Err(e) = db::create_cli_auth_code(&state.pool, &code, expires_at).await {
        tracing::error!("cli_init db error: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let base_url = query.base_url.unwrap_or_else(|| state.public_url.clone());
    let browser_url = format!("{base_url}/cli-auth?code={code}");

    Json(CliInitResponse {
        code,
        browser_url,
        expires_in: CLI_CODE_TTL_SECS,
    })
    .into_response()
}

async fn cli_poll(
    State(state): State<AuthState>,
    Query(query): Query<CliPollQuery>,
) -> impl IntoResponse {
    let now = unix_now();

    let entry = match db::get_cli_auth_code(&state.pool, &query.code).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            return Json(CliPollResponse {
                status: "expired".into(),
                token: None,
            })
            .into_response();
        }
        Err(e) => {
            tracing::error!("cli_poll db error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if entry.expires_at < now {
        return Json(CliPollResponse {
            status: "expired".into(),
            token: None,
        })
        .into_response();
    }

    match entry.status.as_str() {
        "pending" => Json(CliPollResponse {
            status: "pending".into(),
            token: None,
        })
        .into_response(),
        "denied" => Json(CliPollResponse {
            status: "denied".into(),
            token: None,
        })
        .into_response(),
        "authorized" => {
            let account_id = match entry.account_id {
                Some(id) => id,
                None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };

            let raw_token = format!("spwn_tok_{}", Uuid::new_v4().simple());
            let token_hash = hex::encode(Sha256::digest(raw_token.as_bytes()));

            let new_token = db::NewApiToken {
                id: Uuid::new_v4().to_string(),
                account_id,
                token_hash,
                name: format!("CLI ({})", &query.code),
            };

            match db::create_api_token(&state.pool, &new_token).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("cli_poll create token error: {e}");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }

            Json(CliPollResponse {
                status: "authorized".into(),
                token: Some(raw_token),
            })
            .into_response()
        }
        _ => Json(CliPollResponse {
            status: "expired".into(),
            token: None,
        })
        .into_response(),
    }
}

async fn cli_authorize(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(body): Json<CliCodeBody>,
) -> impl IntoResponse {
    let now = unix_now();

    let entry = match db::get_cli_auth_code(&state.pool, &body.code).await {
        Ok(Some(e)) => e,
        Ok(None) => return (StatusCode::NOT_FOUND, "code not found").into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if entry.expires_at < now || entry.status != "pending" {
        return (StatusCode::GONE, "code expired or already used").into_response();
    }

    match db::authorize_cli_auth_code(&state.pool, &body.code, &account_id.0).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn cli_deny(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(body): Json<CliCodeBody>,
) -> impl IntoResponse {
    let _ = account_id;
    match db::deny_cli_auth_code(&state.pool, &body.code).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn resize_avatar(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(data)?;
    let resized = img.resize_to_fill(256, 256, FilterType::Lanczos3);
    let mut buf = Vec::new();
    resized.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)?;
    Ok(buf)
}

async fn signup(
    State(state): State<AuthState>,
    Json(req): Json<SignupRequest>,
) -> impl IntoResponse {
    if req.invite_code != state.invite_code {
        return StatusCode::FORBIDDEN.into_response();
    }

    let username = req.username.to_lowercase();
    if let Err(msg) = crate::validate_username(&username) {
        return (StatusCode::BAD_REQUEST, msg).into_response();
    }

    let hash = match password::hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let now = unix_now();
    let account = db::NewAccount {
        id: Uuid::new_v4().to_string(),
        email: req.email,
        username,
        password_hash: hash,
        created_at: now,
    };

    match db::create_account(&state.pool, &account).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                (StatusCode::BAD_REQUEST, "email or username already taken").into_response()
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

async fn me(State(state): State<AuthState>, account_id: AccountId) -> impl IntoResponse {
    match db::get_account(&state.pool, &account_id.0).await {
        Ok(Some(acc)) => Json(MeResponse {
            id: acc.id,
            email: acc.email,
            username: acc.username,
            display_name: acc.display_name,
            has_avatar: acc.avatar_bytes.is_some(),
            theme: acc.theme,
            vm_limit: acc.vm_limit,
            vcpu_limit: acc.vcpu_limit,
            mem_limit_mb: acc.mem_limit_mb,
        })
        .into_response(),
        Ok(None) => StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn update_theme(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(req): Json<UpdateThemeRequest>,
) -> impl IntoResponse {
    let update = db::UpdateTheme { theme: req.theme };
    match db::update_theme(&state.pool, &account_id.0, &update).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn update_profile(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(req): Json<UpdateProfileRequest>,
) -> impl IntoResponse {
    let acc = match db::get_account(&state.pool, &account_id.0).await {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let update = db::UpdateAccountProfile {
        display_name: req.display_name,
        avatar_bytes: acc.avatar_bytes,
    };

    match db::update_account_profile(&state.pool, &account_id.0, &update).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn upload_avatar(
    State(state): State<AuthState>,
    account_id: AccountId,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let is_image = content_type.starts_with("image/png")
        || content_type.starts_with("image/jpeg")
        || content_type.starts_with("image/webp");

    if !is_image {
        return (StatusCode::BAD_REQUEST, "unsupported image type").into_response();
    }

    if body.len() > 10 * 1024 * 1024 {
        return (StatusCode::BAD_REQUEST, "image too large (max 10mb)").into_response();
    }

    let resized = match resize_avatar(&body) {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, "could not decode image").into_response(),
    };

    let acc = match db::get_account(&state.pool, &account_id.0).await {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let update = db::UpdateAccountProfile {
        display_name: acc.display_name,
        avatar_bytes: Some(resized),
    };

    match db::update_account_profile(&state.pool, &account_id.0, &update).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_avatar(
    State(state): State<AuthState>,
    Path(account_id): Path<String>,
) -> impl IntoResponse {
    let acc = match db::get_account(&state.pool, &account_id).await {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    match acc.avatar_bytes {
        Some(bytes) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
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
pub fn __test_hash(pw: &str) -> anyhow::Result<String> {
    password::hash_password(pw)
}
#[doc(hidden)]
pub fn __test_verify(pw: &str, hash: &str) -> anyhow::Result<bool> {
    password::verify_password(pw, hash)
}
