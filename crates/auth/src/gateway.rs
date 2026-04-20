use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
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
