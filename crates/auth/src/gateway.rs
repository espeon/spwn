use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::routes::AuthState;

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

// TTL for gateway-issued sessions — short-lived, not persisted to browser.
const GATEWAY_SESSION_TTL_SECS: i64 = 24 * 3600; // 24 hours

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[derive(Deserialize)]
pub(crate) struct GatewayAuthPasswordRequest {
    pub(crate) username: String,
    pub(crate) password: String,
}

#[derive(Serialize)]
struct GatewayAuthResponse {
    ok: bool,
    account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(crate) async fn gateway_auth_password(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<GatewayAuthPasswordRequest>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let token_hash = hex::encode(Sha256::digest(req.password.as_bytes()));
    if let Ok(Some(account_id)) = db::get_account_id_by_token_hash(&state.pool, &token_hash).await {
        let _ = db::touch_api_token(&state.pool, &token_hash, unix_now()).await;
        let username = db::get_account(&state.pool, &account_id)
            .await
            .ok()
            .flatten()
            .map(|a| a.username);
        return Json(GatewayAuthResponse {
            ok: true,
            account_id,
            username,
            error: None,
        })
        .into_response();
    }

    let account = match db::get_account_by_email(&state.pool, &req.username).await {
        Ok(Some(a)) => a,
        _ => {
            return Json(GatewayAuthResponse {
                ok: false,
                account_id: String::new(),
                username: None,
                error: Some("invalid credentials".into()),
            })
            .into_response();
        }
    };

    let hash = match &account.password_hash {
        Some(h) => h.clone(),
        None => {
            return Json(GatewayAuthResponse {
                ok: false,
                account_id: String::new(),
                username: None,
                error: Some("invalid credentials".into()),
            })
            .into_response();
        }
    };

    match crate::password::verify_password(&req.password, &hash) {
        Ok(true) => Json(GatewayAuthResponse {
            ok: true,
            account_id: account.id,
            username: Some(account.username),
            error: None,
        })
        .into_response(),
        _ => Json(GatewayAuthResponse {
            ok: false,
            account_id: String::new(),
            username: None,
            error: Some("invalid credentials".into()),
        })
        .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct GatewayAuthPubkeyRequest {
    pub(crate) fingerprint: String,
}

pub(crate) async fn gateway_auth_pubkey(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<GatewayAuthPubkeyRequest>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match db::get_account_id_by_key_fingerprint(&state.pool, &req.fingerprint).await {
        Ok(Some(account_id)) => {
            let username = db::get_account(&state.pool, &account_id)
                .await
                .ok()
                .flatten()
                .map(|a| a.username);
            Json(GatewayAuthResponse {
                ok: true,
                account_id,
                username,
                error: None,
            })
            .into_response()
        }
        _ => Json(GatewayAuthResponse {
            ok: false,
            account_id: String::new(),
            username: None,
            error: Some("unknown key".into()),
        })
        .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct GatewaySessionRequest {
    pub(crate) account_id: String,
}

#[derive(Serialize)]
struct GatewaySessionResponse {
    token: String,
}

pub(crate) async fn gateway_create_session(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(req): Json<GatewaySessionRequest>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let now = unix_now();
    let session = db::NewSession {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: req.account_id,
        created_at: now,
        expires_at: now + GATEWAY_SESSION_TTL_SECS,
    };
    match db::create_session(&state.pool, &session).await {
        Ok(_) => Json(GatewaySessionResponse { token: session.id }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct GatewayLookupVmQuery {
    pub(crate) vm_id: Option<String>,
    pub(crate) subdomain: Option<String>,
}

#[derive(Serialize)]
struct GatewayVmResponse {
    vm_id: String,
    host_agent_addr: String,
    vm_ip: String,
    status: String,
    exposed_port: i32,
}

pub(crate) async fn gateway_lookup_vm(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(q): Query<GatewayLookupVmQuery>,
) -> impl IntoResponse {
    if !check_gateway_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let vm_result = match (&q.vm_id, &q.subdomain) {
        (Some(id), _) => db::get_vm(&state.pool, id).await,
        (_, Some(sub)) => db::get_vm_by_subdomain(&state.pool, sub).await,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "vm_id or subdomain required" })),
            )
                .into_response();
        }
    };
    let vm = match vm_result {
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

/// GET /internal/caddy/auth — Caddy forward_auth endpoint.
/// Checks if a request to a VM subdomain should be allowed.
/// Public VMs are always allowed. Private VMs require a valid session cookie
/// (owner) or a matching share token. Unauthenticated requests are redirected
/// to the login page.
pub(crate) async fn caddy_auth(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Extract subdomain from host (e.g., "debian.spwn.town" → "debian").
    let suffix = format!(".{}", state.base_domain);
    let subdomain = match host.strip_suffix(&suffix) {
        Some(s) => s,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let vm = match db::get_vm_by_subdomain(&state.pool, subdomain).await {
        Ok(Some(vm)) => vm,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    // Public VM — allow everyone.
    if vm.is_public {
        return StatusCode::OK.into_response();
    }

    // Share token — allow if token matches.
    if let Some(token) = params.get("token") {
        if vm.share_token.as_deref() == Some(token.as_str()) {
            return StatusCode::OK.into_response();
        }
    }

    // Auth token — short-lived token from the login flow.
    // Validates the token, creates a session, and sets a cookie on .spwn.town.
    if let Some(auth_token) = params.get("auth_token") {
        if let Ok(Some(token)) = db::consume_vm_auth_token(&state.pool, auth_token).await {
            if token.vm_id == vm.id {
                // Create a session for the user.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let session = db::NewSession {
                    id: uuid::Uuid::new_v4().to_string(),
                    account_id: token.account_id,
                    created_at: now,
                    expires_at: now + state.session_ttl_secs,
                };
                if db::create_session(&state.pool, &session).await.is_ok() {
                    // Set cookie on .spwn.town and allow the request.
                    let cookie = format!(
                        "session_id={}; Path=/; Domain=.{}; HttpOnly; SameSite=Lax{}",
                        session.id,
                        state.base_domain,
                        if state.secure_cookies { "; Secure" } else { "" },
                    );
                    let mut resp = StatusCode::OK.into_response();
                    resp.headers_mut().insert(
                        "set-cookie",
                        cookie.parse().unwrap_or_else(|_| "".parse().unwrap()),
                    );
                    return resp;
                }
            }
        }
    }

    // Session cookie — check if the user owns the VM.
    let cookie_str = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let session_id = cookie_str
        .split(';')
        .map(|c| c.trim())
        .find_map(|c| c.strip_prefix("session_id="));

    if let Some(sid) = session_id {
        if let Ok(Some(session)) = db::get_session(&state.pool, sid).await {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if session.expires_at >= now && session.account_id == vm.account_id {
                return StatusCode::OK.into_response();
            }
        }
    }

    // Not authenticated — redirect to login page.
    let login_url = format!(
        "{}/login?redirect=https://{}",
        state.public_url.trim_end_matches('/'),
        host
    );
    axum::response::Redirect::temporary(&login_url).into_response()
}
