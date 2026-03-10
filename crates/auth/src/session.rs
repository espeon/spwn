use axum::{
    extract::FromRequestParts,
    http::{StatusCode, header, request::Parts},
};
use axum_extra::extract::CookieJar;
use sha2::{Digest, Sha256};

/// Axum extractor that validates the session_id cookie or Authorization: Bearer token
/// and returns the authenticated account id.
/// Requires `db::PgPool` to be present as an Extension on the router.
#[derive(Clone, Debug)]
pub struct AccountId(pub String);

impl<S: Send + Sync> FromRequestParts<S> for AccountId {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pool = parts
            .extensions
            .get::<db::PgPool>()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
            .clone();

        // Try session cookie first.
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(cookie) = jar.get("session_id") {
            let session_id = cookie.value().to_string();
            let now = unix_now();
            let session = db::get_session(&pool, &session_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::UNAUTHORIZED)?;
            if session.expires_at < now {
                return Err(StatusCode::UNAUTHORIZED);
            }
            return Ok(AccountId(session.account_id));
        }

        // Fall back to Authorization: Bearer <token>.
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let raw_token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let token_hash = hex::encode(Sha256::digest(raw_token.as_bytes()));
        let now = unix_now();

        let account_id = db::get_account_id_by_token_hash(&pool, &token_hash)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // Fire-and-forget last_used_at update.
        let pool2 = pool.clone();
        let hash2 = token_hash.clone();
        tokio::spawn(async move {
            let _ = db::touch_api_token(&pool2, &hash2, now).await;
        });

        Ok(AccountId(account_id))
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
