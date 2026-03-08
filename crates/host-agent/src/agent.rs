use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};

use agent_proto::agent::{
    host_agent_server::HostAgent,
    AgentEvent, ConsoleInput, ConsoleOutput,
    CreateVmRequest, CreateVmResponse,
    DeleteVmRequest, DeleteVmResponse,
    RestoreRequest, RestoreResponse,
    StartVmRequest, StartVmResponse,
    StopVmRequest, StopVmResponse,
    TakeSnapshotRequest, TakeSnapshotResponse,
    WatchRequest,
};

use crate::manager::{VmEvent, VmManager};

pub struct HostAgentService {
    pub manager: Arc<VmManager>,
}


#[tonic::async_trait]
impl HostAgent for HostAgentService {
    async fn create_vm(
        &self,
        req: Request<CreateVmRequest>,
    ) -> Result<Response<CreateVmResponse>, Status> {
        let r = req.into_inner();
        match self.manager.create_vm(
            &r.vm_id,
            &r.account_id,
            &r.name,
            &r.subdomain,
            &r.image,
            r.vcores,
            r.memory_mb,
            r.exposed_port,
            &r.ip_address,
        ).await {
            Ok(()) => Ok(Response::new(CreateVmResponse { ok: true, error: String::new() })),
            Err(e) => Ok(Response::new(CreateVmResponse { ok: false, error: e.to_string() })),
        }
    }

    async fn start_vm(
        &self,
        req: Request<StartVmRequest>,
    ) -> Result<Response<StartVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.start_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(StartVmResponse { ok: true, error: String::new() })),
            Err(e) => Ok(Response::new(StartVmResponse { ok: false, error: e.to_string() })),
        }
    }

    async fn stop_vm(
        &self,
        req: Request<StopVmRequest>,
    ) -> Result<Response<StopVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.stop_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(StopVmResponse { ok: true, error: String::new() })),
            Err(e) => Ok(Response::new(StopVmResponse { ok: false, error: e.to_string() })),
        }
    }

    async fn delete_vm(
        &self,
        req: Request<DeleteVmRequest>,
    ) -> Result<Response<DeleteVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.delete_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(DeleteVmResponse { ok: true, error: String::new() })),
            Err(e) => Ok(Response::new(DeleteVmResponse { ok: false, error: e.to_string() })),
        }
    }

    async fn take_snapshot(
        &self,
        req: Request<TakeSnapshotRequest>,
    ) -> Result<Response<TakeSnapshotResponse>, Status> {
        let r = req.into_inner();
        let label = if r.label.is_empty() { None } else { Some(r.label) };
        match self.manager.take_snapshot(&r.vm_id, label).await {
            Ok(snap) => Ok(Response::new(TakeSnapshotResponse {
                ok: true,
                error: String::new(),
                snap_id: snap.id,
                size_bytes: snap.size_bytes,
            })),
            Err(e) => Ok(Response::new(TakeSnapshotResponse {
                ok: false,
                error: e.to_string(),
                snap_id: String::new(),
                size_bytes: 0,
            })),
        }
    }

    async fn restore_snapshot(
        &self,
        req: Request<RestoreRequest>,
    ) -> Result<Response<RestoreResponse>, Status> {
        let r = req.into_inner();
        match self.manager.restore_snapshot(&r.vm_id, &r.snap_id).await {
            Ok(()) => Ok(Response::new(RestoreResponse { ok: true, error: String::new() })),
            Err(e) => Ok(Response::new(RestoreResponse { ok: false, error: e.to_string() })),
        }
    }

    type WatchEventsStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<AgentEvent, Status>> + Send + 'static>,
    >;

    async fn watch_events(
        &self,
        _req: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let rx = self.manager.subscribe_events();
        let stream = BroadcastStream::new(rx).filter_map(|result| {
            let event = result.ok()?;
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let proto = match event {
                VmEvent::Started { vm_id } => AgentEvent {
                    vm_id,
                    event: "started".into(),
                    detail: String::new(),
                    timestamp,
                },
                VmEvent::Stopped { vm_id } => AgentEvent {
                    vm_id,
                    event: "stopped".into(),
                    detail: String::new(),
                    timestamp,
                },
                VmEvent::Crashed { vm_id } => AgentEvent {
                    vm_id,
                    event: "crashed".into(),
                    detail: String::new(),
                    timestamp,
                },
                VmEvent::SnapshotTaken { vm_id, snap_id } => AgentEvent {
                    vm_id,
                    event: "snapshot_taken".into(),
                    detail: snap_id,
                    timestamp,
                },
            };
            Some(Ok(proto))
        });

        Ok(Response::new(Box::pin(stream)))
    }

    type StreamConsoleStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<ConsoleOutput, Status>> + Send + 'static>,
    >;

    async fn stream_console(
        &self,
        _req: Request<Streaming<ConsoleInput>>,
    ) -> Result<Response<Self::StreamConsoleStream>, Status> {
        Err(Status::unimplemented("StreamConsole is reserved for phase 6"))
    }
}
