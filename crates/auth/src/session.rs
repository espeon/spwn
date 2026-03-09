use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use axum_extra::extract::CookieJar;

/// Axum extractor that validates the session_id cookie and returns the account id.
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

        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let session_id = jar
            .get("session_id")
            .map(|c| c.value().to_string())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let now = unix_now();

        let session = db::get_session(&pool, &session_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if session.expires_at < now {
            return Err(StatusCode::UNAUTHORIZED);
        }

        Ok(AccountId(session.account_id))
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
