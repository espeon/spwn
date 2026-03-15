use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use russh::Disconnect;
use russh::client::{self, Config as SshConfig};
use russh::keys::PrivateKeyWithHashAlg;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tonic::{Request, Response, Status, Streaming};

use agent_proto::agent::{
    AgentEvent, BuildImageEvent, BuildImageRequest, CloneVmRequest, CloneVmResponse, ConsoleInput,
    ConsoleOutput, CreateVmRequest, CreateVmResponse, DeleteVmRequest, DeleteVmResponse,
    MigrateVmRequest, MigrateVmResponse, ResizeBandwidthRequest, ResizeBandwidthResponse,
    ResizeCpuRequest, ResizeCpuResponse, RestoreRequest, RestoreResponse, StartVmRequest,
    StartVmResponse, StopVmRequest, StopVmResponse, TakeSnapshotRequest, TakeSnapshotResponse,
    WatchRequest, host_agent_server::HostAgent,
};

use crate::image_build::build_image_inner;
use crate::manager::{VmEvent, VmManager};
use crate::platform_key::{SshClientHandler, load_or_generate_platform_key};

pub struct HostAgentService {
    pub manager: Arc<VmManager>,
    pub agent_secret: String,
}

#[tonic::async_trait]
impl HostAgent for HostAgentService {
    async fn create_vm(
        &self,
        req: Request<CreateVmRequest>,
    ) -> Result<Response<CreateVmResponse>, Status> {
        let r = req.into_inner();
        match self
            .manager
            .create_vm(
                &r.vm_id,
                &r.account_id,
                &r.name,
                &r.subdomain,
                &r.image,
                r.vcpus,
                r.memory_mb,
                r.disk_mb,
                r.bandwidth_mbps,
                r.exposed_port,
                &r.ip_address,
                &r.namespace_id,
            )
            .await
        {
            Ok(()) => Ok(Response::new(CreateVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(CreateVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn start_vm(
        &self,
        req: Request<StartVmRequest>,
    ) -> Result<Response<StartVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.start_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(StartVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(StartVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn stop_vm(
        &self,
        req: Request<StopVmRequest>,
    ) -> Result<Response<StopVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.stop_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(StopVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(StopVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn delete_vm(
        &self,
        req: Request<DeleteVmRequest>,
    ) -> Result<Response<DeleteVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.delete_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(DeleteVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(DeleteVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn take_snapshot(
        &self,
        req: Request<TakeSnapshotRequest>,
    ) -> Result<Response<TakeSnapshotResponse>, Status> {
        let r = req.into_inner();
        let label = if r.label.is_empty() {
            None
        } else {
            Some(r.label)
        };
        match self.manager.take_snapshot(&r.vm_id, label).await {
            Ok(snap) => Ok(Response::new(TakeSnapshotResponse {
                ok: true,
                error: String::new(),
                snap_id: snap.id,
                size_bytes: snap.size_bytes,
            })),
            Err(e) => Ok(Response::new(TakeSnapshotResponse {
                ok: false,
                error: format!("{e:#}"),
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
            Ok(()) => Ok(Response::new(RestoreResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(RestoreResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn clone_vm(
        &self,
        req: Request<CloneVmRequest>,
    ) -> Result<Response<CloneVmResponse>, Status> {
        let r = req.into_inner();
        match self
            .manager
            .clone_vm(
                &r.source_vm_id,
                &r.new_vm_id,
                &r.account_id,
                &r.name,
                &r.subdomain,
                &r.ip_address,
                r.exposed_port,
                r.include_memory,
            )
            .await
        {
            Ok(()) => Ok(Response::new(CloneVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(CloneVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn migrate_vm(
        &self,
        req: Request<MigrateVmRequest>,
    ) -> Result<Response<MigrateVmResponse>, Status> {
        let r = req.into_inner();
        let secret = self.agent_secret.clone();
        match self
            .manager
            .migrate_vm(
                &r.vm_id,
                &r.source_snapshot_url,
                &r.account_id,
                &r.name,
                &r.subdomain,
                r.vcpus,
                r.memory_mb,
                r.disk_mb,
                r.bandwidth_mbps,
                &r.ip_address,
                r.exposed_port,
                &r.image,
                &secret,
                &r.namespace_id,
            )
            .await
        {
            Ok(()) => Ok(Response::new(MigrateVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(MigrateVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn resize_cpu(
        &self,
        req: Request<ResizeCpuRequest>,
    ) -> Result<Response<ResizeCpuResponse>, Status> {
        let r = req.into_inner();
        match self.manager.resize_cpu(&r.vm_id, r.vcpus).await {
            Ok(()) => Ok(Response::new(ResizeCpuResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ResizeCpuResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn resize_bandwidth(
        &self,
        req: Request<ResizeBandwidthRequest>,
    ) -> Result<Response<ResizeBandwidthResponse>, Status> {
        let r = req.into_inner();
        match self
            .manager
            .resize_bandwidth(&r.vm_id, r.bandwidth_mbps)
            .await
        {
            Ok(()) => Ok(Response::new(ResizeBandwidthResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ResizeBandwidthResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    type BuildImageStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<BuildImageEvent, Status>> + Send + 'static>,
    >;

    async fn build_image(
        &self,
        req: Request<BuildImageRequest>,
    ) -> Result<Response<Self::BuildImageStream>, Status> {
        let req = req.into_inner();
        let images_dir = self.manager.images_dir.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            build_image_inner(req, images_dir, tx).await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
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
        req: Request<Streaming<ConsoleInput>>,
    ) -> Result<Response<Self::StreamConsoleStream>, Status> {
        let mut input_stream = req.into_inner();

        // First frame carries vm_id; subsequent frames carry data.
        let first = input_stream
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("stream closed before first frame"))?
            .map_err(|e| Status::internal(e.to_string()))?;

        let vm_id = first.vm_id;
        if vm_id.is_empty() {
            return Err(Status::invalid_argument("first frame must set vm_id"));
        }
        let initial_data = first.data;
        let command = first.command;

        let vm = db::get_vm(&self.manager.pool, &vm_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("vm not found"))?;

        if vm.status != "running" {
            return Err(Status::failed_precondition(format!(
                "vm is {} (must be running)",
                vm.status
            )));
        }

        let key = load_or_generate_platform_key()
            .map_err(|e| Status::internal(format!("platform key: {e}")))?;

        let config = Arc::new(SshConfig::default());
        let mut handle =
            client::connect(config, (vm.ip_address.as_str(), 22u16), SshClientHandler)
                .await
                .map_err(|e| Status::unavailable(format!("ssh connect to vm: {e}")))?;

        let hash_alg = match handle.best_supported_rsa_hash().await {
            Ok(outer) => outer.flatten(),
            Err(_) => None,
        };

        let auth_result = handle
            .authenticate_publickey("root", PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg))
            .await
            .map_err(|e| Status::unauthenticated(format!("ssh auth: {e}")))?;

        if !auth_result.success() {
            return Err(Status::unauthenticated(
                "ssh authentication failed — ensure platform public key is in vm's authorized_keys",
            ));
        }

        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| Status::internal(format!("open session: {e}")))?;

        if command.is_empty() {
            channel
                .request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
                .await
                .map_err(|e| Status::internal(format!("pty request: {e}")))?;

            channel
                .request_shell(false)
                .await
                .map_err(|e| Status::internal(format!("shell request: {e}")))?;
        } else {
            channel
                .exec(false, command.as_str())
                .await
                .map_err(|e| Status::internal(format!("exec request: {e}")))?;
        }

        let ssh_stream = channel.into_stream();
        let (mut ssh_reader, mut ssh_writer) = tokio::io::split(ssh_stream);

        let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Result<ConsoleOutput, Status>>(64);
        let out_stream = ReceiverStream::new(out_rx);

        // SSH → gRPC output
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match ssh_reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx
                            .send(Ok(ConsoleOutput {
                                data: buf[..n].to_vec(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            let _ = handle.disconnect(Disconnect::ByApplication, "", "").await;
        });

        // gRPC input → SSH
        tokio::spawn(async move {
            if !initial_data.is_empty() {
                let _ = ssh_writer.write_all(&initial_data).await;
            }
            while let Some(Ok(frame)) = input_stream.next().await {
                if frame.data.is_empty() {
                    continue;
                }
                if ssh_writer.write_all(&frame.data).await.is_err() {
                    break;
                }
            }
            let _ = ssh_writer.shutdown().await;
        });

        Ok(Response::new(Box::pin(out_stream)))
    }
}
