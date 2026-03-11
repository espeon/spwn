use tonic::{Request, Response, Status};

use agent_proto::agent::{
    control_plane_server::ControlPlane,
    HeartbeatRequest, HeartbeatResponse,
    RegisterRequest, RegisterResponse,
};

use crate::events::EventWatcher;

pub struct ControlPlaneService {
    pub pool: db::PgPool,
    pub event_watcher: EventWatcher,
}

#[tonic::async_trait]
impl ControlPlane for ControlPlaneService {
    async fn register(
        &self,
        req: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let r = req.into_inner();
        let host = db::NewHost {
            id: r.host_id.clone(),
            name: r.name,
            address: r.address.clone(),
            vcpu_total: r.vcpu_total as i64,
            mem_total_mb: r.mem_total_mb as i32,
            images_dir: r.images_dir,
            overlay_dir: r.overlay_dir,
            snapshot_dir: r.snapshot_dir,
            kernel_path: r.kernel_path,
            snapshot_addr: r.snapshot_addr,
        };
        match db::upsert_host(&self.pool, &host).await {
            Ok(_) => {
                tracing::info!("host {} registered ({})", r.host_id, r.address);
                self.event_watcher.watch_host(r.host_id, r.address).await;
                Ok(Response::new(RegisterResponse { ok: true }))
            }
            Err(e) => {
                tracing::error!("failed to register host {}: {e}", r.host_id);
                Ok(Response::new(RegisterResponse { ok: false }))
            }
        }
    }

    async fn heartbeat(
        &self,
        req: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let r = req.into_inner();
        db::update_host_heartbeat(
            &self.pool,
            &r.host_id,
            r.vcpu_used as i64,
            r.mem_used_mb as i32,
        )
        .await
        .ok();
        Ok(Response::new(HeartbeatResponse {}))
    }
}
