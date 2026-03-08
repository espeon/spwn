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

    /// Write the stopped page template to disk. Call once at startup before any routes are set.
    pub fn write_static_files(&self) -> Result<()> {
        std::fs::create_dir_all(&self.static_files_path)?;
        std::fs::write(self.static_files_path.join("stopped.html"), STOPPED_HTML)?;
        Ok(())
    }

    pub async fn health(&self) -> Result<()> {
        let resp = self.client.get(format!("{}/config/", self.base)).send().await?;
        check(resp).await.map(|_| ())
    }

    /// Set or replace the route for a running VM.
    pub async fn set_vm_route(&self, subdomain: &str, vm_ip: &str, port: u16) -> Result<()> {
        let route = vm_route(subdomain, vm_ip, port);
        self.upsert_route(subdomain, route).await
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
        self.patch("/config/apps/http/servers/main/routes", &array).await?;
        Ok(())
    }

    // Try PUT /id/route-<subdomain> first. On 404 (route doesn't exist yet), POST to append.
    async fn upsert_route(&self, subdomain: &str, route: Value) -> Result<()> {
        let id_url = format!("{}/id/route-{subdomain}", self.base);
        let resp = self.client.put(&id_url).json(&route).send().await?;
        if resp.status() == 404 {
            self.post("/config/apps/http/servers/main/routes/", &route).await?;
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
