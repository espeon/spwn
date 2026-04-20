use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::{routes::AuthState, session::AccountId};

const CHALLENGE_TTL_SECS: i64 = 300;
pub(crate) const PENDING_AUTH_TTL_SECS: i64 = 300;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

// ── registration ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct RegisterStartResponse {
    challenge_id: String,
    #[serde(flatten)]
    ccr: CreationChallengeResponse,
}

pub async fn passkey_register_start(
    State(state): State<AuthState>,
    account_id: AccountId,
) -> impl IntoResponse {
    let account = match db::get_account(&state.pool, &account_id.0).await {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let existing = match db::list_passkeys(&state.pool, &account_id.0).await {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let exclude: Vec<CredentialID> = existing
        .iter()
        .map(|p| CredentialID::from(p.credential_id.clone()))
        .collect();

    let user_uuid = match Uuid::parse_str(&account_id.0) {
        Ok(u) => u,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let display_name = account
        .display_name
        .as_deref()
        .unwrap_or(&account.username);

    let (ccr, reg_state) = match state.webauthn.start_passkey_registration(
        user_uuid,
        &account.username,
        display_name,
        Some(exclude),
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("start_passkey_registration error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let challenge_json = match serde_json::to_string(&reg_state) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let challenge_id = Uuid::new_v4().to_string();
    let expires_at = unix_now() + CHALLENGE_TTL_SECS;

    if let Err(e) = db::create_passkey_challenge(
        &state.pool,
        &challenge_id,
        Some(&account_id.0),
        &challenge_json,
        "registration",
        expires_at,
    )
    .await
    {
        tracing::error!("create_passkey_challenge error: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(RegisterStartResponse { challenge_id, ccr }).into_response()
}

#[derive(Deserialize)]
pub struct RegisterFinishRequest {
    challenge_id: String,
    name: String,
    credential: RegisterPublicKeyCredential,
}

pub async fn passkey_register_finish(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(req): Json<RegisterFinishRequest>,
) -> impl IntoResponse {
    let challenge = match db::get_passkey_challenge(&state.pool, &req.challenge_id, "registration").await {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::BAD_REQUEST, "challenge not found or expired").into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if challenge.expires_at < unix_now() {
        let _ = db::delete_passkey_challenge(&state.pool, &req.challenge_id).await;
        return (StatusCode::BAD_REQUEST, "challenge expired").into_response();
    }

    if challenge.account_id.as_deref() != Some(&account_id.0) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let reg_state: PasskeyRegistration = match serde_json::from_str(&challenge.challenge_json) {
        Ok(s) => s,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let _ = db::delete_passkey_challenge(&state.pool, &req.challenge_id).await;

    let passkey = match state.webauthn.finish_passkey_registration(&req.credential, &reg_state) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("finish_passkey_registration error: {e}");
            return (StatusCode::BAD_REQUEST, "registration verification failed").into_response();
        }
    };

    let passkey_json = match serde_json::to_string(&passkey) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let cred_id: Vec<u8> = passkey.cred_id().to_vec();
    let passkey_id = Uuid::new_v4().to_string();

    match db::create_passkey(
        &state.pool,
        &passkey_id,
        &account_id.0,
        &cred_id,
        &passkey_json,
        &req.name,
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                return (StatusCode::CONFLICT, "passkey already registered").into_response();
            }
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    #[derive(Serialize)]
    struct RegisterFinishResponse {
        id: String,
        name: String,
    }

    Json(RegisterFinishResponse {
        id: passkey_id,
        name: req.name,
    })
    .into_response()
}

// ── passkey management ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PasskeyItem {
    pub id: String,
    pub name: String,
    pub created_at: i64,
}

pub async fn list_passkeys(
    State(state): State<AuthState>,
    account_id: AccountId,
) -> impl IntoResponse {
    match db::list_passkeys(&state.pool, &account_id.0).await {
        Ok(keys) => {
            let items: Vec<PasskeyItem> = keys
                .into_iter()
                .map(|k| PasskeyItem {
                    id: k.id,
                    name: k.name,
                    created_at: k.created_at,
                })
                .collect();
            Json(items).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct RenamePasskeyRequest {
    name: String,
}

pub async fn rename_passkey(
    State(state): State<AuthState>,
    account_id: AccountId,
    Path(id): Path<String>,
    Json(req): Json<RenamePasskeyRequest>,
) -> impl IntoResponse {
    match db::rename_passkey(&state.pool, &id, &account_id.0, &req.name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn delete_passkey(
    State(state): State<AuthState>,
    account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::delete_passkey(&state.pool, &id, &account_id.0).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ── auth mode management ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateAuthModeRequest {
    mode: String,
}

pub async fn update_auth_mode(
    State(state): State<AuthState>,
    account_id: AccountId,
    Json(req): Json<UpdateAuthModeRequest>,
) -> impl IntoResponse {
    let valid_modes = ["password", "passkey", "password_passkey"];
    if !valid_modes.contains(&req.mode.as_str()) {
        return (StatusCode::BAD_REQUEST, "invalid auth mode").into_response();
    }

    if req.mode == "passkey" || req.mode == "password_passkey" {
        let passkeys = match db::list_passkeys(&state.pool, &account_id.0).await {
            Ok(p) => p,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        if passkeys.is_empty() {
            return (StatusCode::BAD_REQUEST, "register a passkey before switching mode").into_response();
        }
    }

    match db::set_account_auth_mode(&state.pool, &account_id.0, &req.mode).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ── passkey login ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginStartRequest {
    username: String,
}

#[derive(Serialize)]
pub struct LoginStartResponse {
    challenge_id: String,
    #[serde(flatten)]
    rcr: RequestChallengeResponse,
}

pub async fn passkey_login_start(
    State(state): State<AuthState>,
    Json(req): Json<LoginStartRequest>,
) -> impl IntoResponse {
    let account = match db::get_account_by_username(&state.pool, &req.username).await {
        Ok(Some(a)) => a,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if account.auth_mode == "password" {
        return (StatusCode::BAD_REQUEST, "account does not use passkeys").into_response();
    }

    let passkey_rows = match db::list_passkeys(&state.pool, &account.id).await {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if passkey_rows.is_empty() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let passkeys: Vec<Passkey> = match passkey_rows
        .iter()
        .map(|r| serde_json::from_str::<Passkey>(&r.passkey_json))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("deserialize passkey error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let (rcr, auth_state) = match state.webauthn.start_passkey_authentication(&passkeys) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("start_passkey_authentication error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let challenge_json = match serde_json::to_string(&auth_state) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let challenge_id = Uuid::new_v4().to_string();
    let expires_at = unix_now() + CHALLENGE_TTL_SECS;

    if let Err(e) = db::create_passkey_challenge(
        &state.pool,
        &challenge_id,
        Some(&account.id),
        &challenge_json,
        "login",
        expires_at,
    )
    .await
    {
        tracing::error!("create_passkey_challenge error: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(LoginStartResponse { challenge_id, rcr }).into_response()
}

#[derive(Deserialize)]
pub struct LoginFinishRequest {
    challenge_id: String,
    credential: PublicKeyCredential,
}

pub async fn passkey_login_finish(
    State(state): State<AuthState>,
    jar: axum_extra::extract::cookie::CookieJar,
    Json(req): Json<LoginFinishRequest>,
) -> impl IntoResponse {
    let challenge = match db::get_passkey_challenge(&state.pool, &req.challenge_id, "login").await {
        Ok(Some(c)) => c,
        Ok(None) => return (jar, StatusCode::BAD_REQUEST).into_response(),
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    if challenge.expires_at < unix_now() {
        let _ = db::delete_passkey_challenge(&state.pool, &req.challenge_id).await;
        return (jar, StatusCode::BAD_REQUEST).into_response();
    }

    let account_id = match &challenge.account_id {
        Some(id) => id.clone(),
        None => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    let auth_state: PasskeyAuthentication = match serde_json::from_str(&challenge.challenge_json) {
        Ok(s) => s,
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    let _ = db::delete_passkey_challenge(&state.pool, &req.challenge_id).await;

    let auth_result = match state.webauthn.finish_passkey_authentication(&req.credential, &auth_state) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("finish_passkey_authentication error: {e}");
            return (jar, StatusCode::UNAUTHORIZED).into_response();
        }
    };

    // Update the passkey counter if needed.
    let cred_id_bytes: Vec<u8> = auth_result.cred_id().to_vec();
    if let Ok(Some(row)) = db::get_passkey_by_credential_id(&state.pool, &cred_id_bytes).await
        && let Ok(mut passkey) = serde_json::from_str::<Passkey>(&row.passkey_json)
            && passkey.update_credential(&auth_result) == Some(true)
                && let Ok(updated_json) = serde_json::to_string(&passkey) {
                    let _ = db::update_passkey_json(&state.pool, &row.id, &updated_json).await;
                }

    match create_session(&state, jar, &account_id).await {
        Ok(jar) => (jar, StatusCode::OK).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ── 2FA verify (password_passkey mode) ────────────────────────────────────────

#[derive(Deserialize)]
pub struct PasskeyVerifyRequest {
    pending_token: String,
    challenge_id: String,
    credential: PublicKeyCredential,
}

pub async fn passkey_verify(
    State(state): State<AuthState>,
    jar: axum_extra::extract::cookie::CookieJar,
    Json(req): Json<PasskeyVerifyRequest>,
) -> impl IntoResponse {
    let token = match db::get_pending_auth_token(&state.pool, &req.pending_token).await {
        Ok(Some(t)) => t,
        Ok(None) => return (jar, StatusCode::UNAUTHORIZED).into_response(),
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    if token.expires_at < unix_now() {
        let _ = db::delete_pending_auth_token(&state.pool, &req.pending_token).await;
        return (jar, StatusCode::UNAUTHORIZED).into_response();
    }

    let challenge = match db::get_passkey_challenge(&state.pool, &req.challenge_id, "login").await {
        Ok(Some(c)) => c,
        Ok(None) => return (jar, StatusCode::BAD_REQUEST).into_response(),
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    if challenge.expires_at < unix_now() {
        let _ = db::delete_passkey_challenge(&state.pool, &req.challenge_id).await;
        return (jar, StatusCode::BAD_REQUEST).into_response();
    }

    if challenge.account_id.as_deref() != Some(&token.account_id) {
        return (jar, StatusCode::FORBIDDEN).into_response();
    }

    let auth_state: PasskeyAuthentication = match serde_json::from_str(&challenge.challenge_json) {
        Ok(s) => s,
        Err(_) => return (jar, StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    let _ = db::delete_passkey_challenge(&state.pool, &req.challenge_id).await;
    let _ = db::delete_pending_auth_token(&state.pool, &req.pending_token).await;

    let auth_result = match state.webauthn.finish_passkey_authentication(&req.credential, &auth_state) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("passkey_verify finish error: {e}");
            return (jar, StatusCode::UNAUTHORIZED).into_response();
        }
    };

    let cred_id_bytes: Vec<u8> = auth_result.cred_id().to_vec();
    if let Ok(Some(row)) = db::get_passkey_by_credential_id(&state.pool, &cred_id_bytes).await
        && let Ok(mut passkey) = serde_json::from_str::<Passkey>(&row.passkey_json)
            && passkey.update_credential(&auth_result) == Some(true)
                && let Ok(updated_json) = serde_json::to_string(&passkey) {
                    let _ = db::update_passkey_json(&state.pool, &row.id, &updated_json).await;
                }

    match create_session(&state, jar, &token.account_id).await {
        Ok(jar) => (jar, StatusCode::OK).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ── 2FA challenge start (for password_passkey mode after password succeeds) ───

pub async fn passkey_2fa_start(
    State(state): State<AuthState>,
    Json(req): Json<PendingTokenRequest>,
) -> impl IntoResponse {
    let token = match db::get_pending_auth_token(&state.pool, &req.pending_token).await {
        Ok(Some(t)) => t,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if token.expires_at < unix_now() {
        let _ = db::delete_pending_auth_token(&state.pool, &req.pending_token).await;
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let passkey_rows = match db::list_passkeys(&state.pool, &token.account_id).await {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let passkeys: Vec<Passkey> = match passkey_rows
        .iter()
        .map(|r| serde_json::from_str::<Passkey>(&r.passkey_json))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let (rcr, auth_state) = match state.webauthn.start_passkey_authentication(&passkeys) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("start_passkey_authentication error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let challenge_json = match serde_json::to_string(&auth_state) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let challenge_id = Uuid::new_v4().to_string();
    let expires_at = unix_now() + CHALLENGE_TTL_SECS;

    if let Err(e) = db::create_passkey_challenge(
        &state.pool,
        &challenge_id,
        Some(&token.account_id),
        &challenge_json,
        "login",
        expires_at,
    )
    .await
    {
        tracing::error!("create_passkey_challenge error: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(LoginStartResponse { challenge_id, rcr }).into_response()
}

#[derive(Deserialize)]
pub struct PendingTokenRequest {
    pub pending_token: String,
}

// ── helpers ───────────────────────────────────────────────────────────────────

async fn create_session(
    state: &AuthState,
    jar: axum_extra::extract::cookie::CookieJar,
    account_id: &str,
) -> Result<axum_extra::extract::cookie::CookieJar, ()> {
    use axum_extra::extract::cookie::{Cookie, SameSite};

    let now = unix_now();
    let session = db::NewSession {
        id: Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        created_at: now,
        expires_at: now + state.session_ttl_secs,
    };

    db::create_session(&state.pool, &session).await.map_err(|_| ())?;

    let cookie = Cookie::build(("session_id", session.id))
        .http_only(true)
        .same_site(if state.secure_cookies { SameSite::None } else { SameSite::Lax })
        .secure(state.secure_cookies)
        .path("/")
        .build();

    Ok(jar.add(cookie))
}
