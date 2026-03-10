use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, patch, post},
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
    pub gateway_secret: Option<String>,
    pub ssh_gateway_addr: String,
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
        .route("/api/config", get(server_config))
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
        .route("/api/account/keys", get(list_ssh_keys).post(add_ssh_key))
        .route("/api/account/keys/{id}", delete(delete_ssh_key))
        .route(
            "/internal/gateway/auth/password",
            post(gateway_auth_password),
        )
        .route("/internal/gateway/auth/pubkey", post(gateway_auth_pubkey))
        .route("/internal/gateway/vm", get(gateway_lookup_vm))
        .with_state(state)
}

#[derive(Serialize)]
struct ServerConfig {
    ssh_gateway_addr: String,
}

async fn server_config(State(state): State<AuthState>) -> impl IntoResponse {
    Json(ServerConfig {
        ssh_gateway_addr: state.ssh_gateway_addr.clone(),
    })
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

// ── SSH keys ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct SshKeyResponse {
    id: String,
    name: String,
    fingerprint: String,
    created_at: i64,
}

impl From<db::SshKeyRow> for SshKeyResponse {
    fn from(r: db::SshKeyRow) -> Self {
        SshKeyResponse {
            id: r.id,
            name: r.name,
            fingerprint: r.fingerprint,
            created_at: r.created_at,
        }
    }
}

async fn list_ssh_keys(State(state): State<AuthState>, account_id: AccountId) -> impl IntoResponse {
    match db::list_ssh_keys(&state.pool, &account_id.0).await {
        Ok(keys) => {
            let resp: Vec<SshKeyResponse> = keys.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct AddSshKeyRequest {
    name: String,
    public_key: String,
}

async fn add_ssh_key(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(req): Json<AddSshKeyRequest>,
) -> impl IntoResponse {
    let fingerprint = match ssh_key_fingerprint(&req.public_key) {
        Ok(fp) => fp,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid public key" })),
            )
                .into_response();
        }
    };
    match db::add_ssh_key(
        &state.pool,
        &account_id.0,
        &req.name,
        &req.public_key,
        &fingerprint,
    )
    .await
    {
        Ok(key) => (StatusCode::CREATED, Json(SshKeyResponse::from(key))).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "error": "key already added" })),
                )
                    .into_response();
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response()
        }
    }
}

async fn delete_ssh_key(
    State(state): State<AuthState>,
    account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::delete_ssh_key(&state.pool, &id, &account_id.0).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "key not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn ssh_key_fingerprint(public_key: &str) -> anyhow::Result<String> {
    use base64::Engine;
    let parts: Vec<&str> = public_key.split_whitespace().collect();
    let b64 = parts
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("missing key data"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let hash = Sha256::digest(&bytes);
    Ok(format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(hash)
    ))
}

// ── Internal gateway endpoints ────────────────────────────────────────────────

fn check_gateway_secret(state: &AuthState, headers: &HeaderMap) -> bool {
    let secret = match &state.gateway_secret {
        Some(s) => s,
        None => return false,
    };
    let auth = match headers.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(v) => v,
        None => return false,
    };
    let token = auth.strip_prefix("Bearer ").unwrap_or(auth);
    token == secret
}

#[derive(Deserialize)]
struct GatewayAuthPasswordRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct GatewayAuthResponse {
    ok: bool,
    account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn gateway_auth_password(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<GatewayAuthPasswordRequest>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // try bearer token first
    let token_hash = hex::encode(Sha256::digest(req.password.as_bytes()));
    if let Ok(Some(account_id)) = db::get_account_id_by_token_hash(&state.pool, &token_hash).await {
        let _ = db::touch_api_token(&state.pool, &token_hash, unix_now()).await;
        return Json(GatewayAuthResponse {
            ok: true,
            account_id,
            error: None,
        })
        .into_response();
    }

    // try account password (username is email)
    let account = match db::get_account_by_email(&state.pool, &req.username).await {
        Ok(Some(a)) => a,
        _ => {
            return Json(GatewayAuthResponse {
                ok: false,
                account_id: String::new(),
                error: Some("invalid credentials".into()),
            })
            .into_response();
        }
    };

    match password::verify_password(&req.password, &account.password_hash) {
        Ok(true) => Json(GatewayAuthResponse {
            ok: true,
            account_id: account.id,
            error: None,
        })
        .into_response(),
        _ => Json(GatewayAuthResponse {
            ok: false,
            account_id: String::new(),
            error: Some("invalid credentials".into()),
        })
        .into_response(),
    }
}

#[derive(Deserialize)]
struct GatewayAuthPubkeyRequest {
    fingerprint: String,
}

async fn gateway_auth_pubkey(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<GatewayAuthPubkeyRequest>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match db::get_account_id_by_key_fingerprint(&state.pool, &req.fingerprint).await {
        Ok(Some(account_id)) => Json(GatewayAuthResponse {
            ok: true,
            account_id,
            error: None,
        })
        .into_response(),
        _ => Json(GatewayAuthResponse {
            ok: false,
            account_id: String::new(),
            error: Some("unknown key".into()),
        })
        .into_response(),
    }
}

#[derive(Deserialize)]
struct GatewayLookupVmQuery {
    vm_id: String,
}

#[derive(Serialize)]
struct GatewayVmResponse {
    vm_id: String,
    host_agent_addr: String,
    vm_ip: String,
    status: String,
    exposed_port: i32,
}

async fn gateway_lookup_vm(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(q): Query<GatewayLookupVmQuery>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let vm = match db::get_vm(&state.pool, &q.vm_id).await {
        Ok(Some(v)) => v,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "vm not found" })),
            )
                .into_response();
        }
    };
    let host_id = match &vm.host_id {
        Some(id) => id.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "vm has no host assigned" })),
            )
                .into_response();
        }
    };
    let host = match db::get_host(&state.pool, &host_id).await {
        Ok(Some(h)) => h,
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "host not found" })),
            )
                .into_response();
        }
    };
    Json(GatewayVmResponse {
        vm_id: vm.id,
        host_agent_addr: host.address,
        vm_ip: vm.ip_address,
        status: vm.status,
        exposed_port: vm.exposed_port,
    })
    .into_response()
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
