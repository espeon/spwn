use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaddyError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("caddy returned {status}: {body}")]
    ApiError { status: u16, body: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, CaddyError>;

#[derive(Clone)]
pub struct CaddyClient {
    base: String,
    client: reqwest::Client,
    static_files_path: PathBuf,
}

pub struct RouteEntry {
    pub subdomain: String,
    pub target: RouteTarget,
}

pub enum RouteTarget {
    Vm { ip: String, port: u16 },
    Stopped,
}

static STOPPED_HTML: &str = include_str!("stopped.html");

impl CaddyClient {
    pub fn new(base_url: &str, static_files_path: PathBuf) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            static_files_path,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Write the stopped page template to disk. Call once at startup before any routes are set.
    pub fn write_static_files(&self) -> Result<()> {
        std::fs::create_dir_all(&self.static_files_path)?;
        std::fs::write(self.static_files_path.join("stopped.html"), STOPPED_HTML)?;
        Ok(())
    }

    pub async fn health(&self) -> Result<()> {
        let resp = self
            .client
            .get(format!("{}/config/", self.base))
            .send()
            .await?;
        check(resp).await.map(|_| ())
    }

    /// Set or replace the route for a running VM.
    pub async fn set_vm_route(&self, subdomain: &str, vm_ip: &str, port: u16) -> Result<()> {
        let route = vm_route(subdomain, vm_ip, port);
        self.upsert_route(subdomain, route).await
    }

    /// Delete a route by subdomain. Ignores 404 (already gone).
    pub async fn delete_route(&self, subdomain: &str) -> Result<()> {
        let id_url = format!("{}/id/route-{subdomain}", self.base);
        let resp = self.client.delete(&id_url).send().await?;
        if resp.status() == 404 {
            return Ok(());
        }
        check(resp).await.map(|_| ())
    }

    /// Set or replace the route for a stopped VM (serves a static 503 page).
    pub async fn set_stopped_route(&self, subdomain: &str) -> Result<()> {
        let route = self.stopped_route(subdomain)?;
        self.upsert_route(subdomain, route).await
    }

    /// Rebuild the full routes array from a snapshot (used on startup / reconciliation).
    pub async fn rebuild_all_routes(&self, routes: &[RouteEntry]) -> Result<()> {
        let array = routes
            .iter()
            .map(|r| match &r.target {
                RouteTarget::Vm { ip, port } => Ok(vm_route(&r.subdomain, ip, *port)),
                RouteTarget::Stopped => self.stopped_route(&r.subdomain),
            })
            .collect::<Result<Vec<Value>>>()?;
        self.patch("/config/apps/http/servers/main/routes", &array)
            .await?;
        Ok(())
    }

    // Try PUT /id/route-<subdomain> first. On 404 (route doesn't exist yet), POST to append.
    async fn upsert_route(&self, subdomain: &str, route: Value) -> Result<()> {
        let id_url = format!("{}/id/route-{subdomain}", self.base);
        let resp = self.client.put(&id_url).json(&route).send().await?;
        if resp.status() == 404 {
            self.post("/config/apps/http/servers/main/routes/", &route)
                .await?;
        } else {
            check(resp).await?;
        }
        Ok(())
    }

    fn stopped_route(&self, subdomain: &str) -> Result<Value> {
        let template = std::fs::read_to_string(self.static_files_path.join("stopped.html"))?;
        let body = template.replace("{http.request.host}", subdomain);
        Ok(json!({
            "@id": format!("route-{subdomain}"),
            "match": [{"host": [subdomain]}],
            "handle": [{
                "handler": "static_response",
                "status_code": 503,
                "body": body,
                "headers": {"Content-Type": ["text/html; charset=utf-8"]}
            }]
        }))
    }

    async fn patch<T: Serialize>(&self, path: &str, body: &T) -> Result<Value> {
        let resp = self
            .client
            .patch(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await?;
        check(resp).await
    }

    async fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<Value> {
        let resp = self
            .client
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await?;
        check(resp).await
    }
}

fn vm_route(subdomain: &str, vm_ip: &str, port: u16) -> Value {
    json!({
        "@id": format!("route-{subdomain}"),
        "match": [{"host": [subdomain]}],
        "handle": [{
            "handler": "reverse_proxy",
            "upstreams": [{"dial": format!("{vm_ip}:{port}")}],
            "flush_interval": -1,
            "transport": {
                "protocol": "http",
                "read_timeout": "300s",
                "write_timeout": "300s"
            }
        }]
    })
}

async fn check(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(CaddyError::ApiError {
            status: status.as_u16(),
            body,
        });
    }
    Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use axum::{Router, http::StatusCode, routing::{post, put}};
    use tempfile::tempdir;

    async fn start_mock(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    fn ok_server() -> Router {
        Router::new().fallback(|| async { (StatusCode::OK, "null") })
    }

    fn err_server() -> Router {
        Router::new().fallback(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "oops") })
    }

    fn make_client(base: &str) -> CaddyClient {
        CaddyClient::new(base, std::env::temp_dir())
    }

    fn make_client_with_dir(base: &str, dir: &std::path::Path) -> CaddyClient {
        CaddyClient::new(base, dir.to_path_buf())
    }

    #[test]
    fn base_url_roundtrips() {
        let client = make_client("http://localhost:2019");
        assert_eq!(client.base_url(), "http://localhost:2019");
    }

    #[test]
    fn base_url_strips_trailing_slash() {
        let client = make_client("http://localhost:2019/");
        assert_eq!(client.base_url(), "http://localhost:2019");
    }

    #[test]
    fn write_static_files_creates_stopped_html() {
        let dir = tempdir().unwrap();
        let client = CaddyClient::new("http://localhost:9999", dir.path().to_path_buf());
        client.write_static_files().unwrap();
        let content = std::fs::read_to_string(dir.path().join("stopped.html")).unwrap();
        assert!(content.contains("{http.request.host}"));
    }

    #[tokio::test]
    async fn health_ok() {
        let base = start_mock(ok_server()).await;
        assert!(make_client(&base).health().await.is_ok());
    }

    #[tokio::test]
    async fn health_err_on_server_error() {
        let base = start_mock(err_server()).await;
        assert!(make_client(&base).health().await.is_err());
    }

    #[tokio::test]
    async fn set_vm_route_success() {
        let base = start_mock(ok_server()).await;
        make_client(&base)
            .set_vm_route("app.user", "172.16.1.2", 8080)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn set_vm_route_falls_back_to_post_on_404() {
        let posted = Arc::new(AtomicBool::new(false));
        let posted2 = posted.clone();

        let router = Router::new()
            .route(
                "/id/route-app.user",
                put(|| async { (StatusCode::NOT_FOUND, "null") }),
            )
            .route(
                "/config/apps/http/servers/main/routes/",
                post(move || {
                    posted.store(true, Ordering::SeqCst);
                    async { (StatusCode::OK, "null") }
                }),
            );

        let base = start_mock(router).await;
        make_client(&base)
            .set_vm_route("app.user", "172.16.1.2", 8080)
            .await
            .unwrap();
        assert!(posted2.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn delete_route_success() {
        let base = start_mock(ok_server()).await;
        make_client(&base).delete_route("app.user").await.unwrap();
    }

    #[tokio::test]
    async fn delete_route_ignores_404() {
        let router = Router::new()
            .fallback(|| async { (StatusCode::NOT_FOUND, "null") });
        let base = start_mock(router).await;
        assert!(make_client(&base).delete_route("app.user").await.is_ok());
    }

    #[tokio::test]
    async fn delete_route_propagates_other_errors() {
        let base = start_mock(err_server()).await;
        assert!(make_client(&base).delete_route("app.user").await.is_err());
    }

    #[tokio::test]
    async fn set_stopped_route_success() {
        let dir = tempdir().unwrap();
        let base = start_mock(ok_server()).await;
        let client = make_client_with_dir(&base, dir.path());
        client.write_static_files().unwrap();
        client.set_stopped_route("app.user").await.unwrap();
    }

    #[tokio::test]
    async fn rebuild_all_routes_success() {
        let dir = tempdir().unwrap();
        let base = start_mock(ok_server()).await;
        let client = make_client_with_dir(&base, dir.path());
        client.write_static_files().unwrap();
        let routes = vec![
            RouteEntry {
                subdomain: "vm1.user".into(),
                target: RouteTarget::Vm { ip: "172.16.1.2".into(), port: 8080 },
            },
            RouteEntry {
                subdomain: "vm2.user".into(),
                target: RouteTarget::Stopped,
            },
        ];
        client.rebuild_all_routes(&routes).await.unwrap();
    }

    #[tokio::test]
    async fn rebuild_all_routes_propagates_server_error() {
        let base = start_mock(err_server()).await;
        let routes = vec![RouteEntry {
            subdomain: "vm1.user".into(),
            target: RouteTarget::Vm { ip: "172.16.1.2".into(), port: 8080 },
        }];
        assert!(make_client(&base).rebuild_all_routes(&routes).await.is_err());
    }

    #[tokio::test]
    async fn check_returns_null_on_empty_ok_body() {
        let router = Router::new().fallback(|| async { (StatusCode::OK, "") });
        let base = start_mock(router).await;
        let client = reqwest::Client::new();
        let resp = client.get(format!("{base}/")).send().await.unwrap();
        let result = check(resp).await.unwrap();
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn write_static_files_err_on_invalid_path() {
        let client = CaddyClient::new(
            "http://localhost:9999",
            std::path::PathBuf::from("/nonexistent/invalid/path/xyz"),
        );
        assert!(client.write_static_files().is_err());
    }
}
