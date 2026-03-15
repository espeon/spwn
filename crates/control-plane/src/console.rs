use axum::{
    Extension,
    extract::{
        Path, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt, stream::{SplitSink, SplitStream}};
use tonic::transport::Channel;
use tracing::error;

use agent_proto::agent::{ConsoleInput, host_agent_client::HostAgentClient};
use auth::AccountId;

pub async fn vm_console(
    Path(vm_id): Path<String>,
    account_id: AccountId,
    ws: WebSocketUpgrade,
    Extension(pool): Extension<db::PgPool>,
) -> impl IntoResponse {
    let vm = match db::get_vm(&pool, &vm_id).await {
        Ok(Some(v)) => v,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    if vm.account_id != account_id.0 {
        return StatusCode::FORBIDDEN.into_response();
    }

    let host_id = match &vm.host_id {
        Some(id) => id.clone(),
        None => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    let host = match db::get_host(&pool, &host_id).await {
        Ok(Some(h)) => h,
        _ => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    ws.on_upgrade(move |socket| relay(socket, vm_id, host.address))
        .into_response()
}

async fn relay(ws: WebSocket, vm_id: String, host_addr: String) {
    let channel = match Channel::from_shared(host_addr) {
        Ok(c) => match c.connect().await {
            Ok(ch) => ch,
            Err(e) => {
                error!("console: failed to connect to host agent: {e}");
                return;
            }
        },
        Err(e) => {
            error!("console: invalid host address: {e}");
            return;
        }
    };

    let mut agent = HostAgentClient::new(channel);

    let (grpc_tx, grpc_rx) = tokio::sync::mpsc::channel::<ConsoleInput>(32);

    // First frame identifies the VM.
    if grpc_tx
        .send(ConsoleInput {
            vm_id: vm_id.clone(),
            data: vec![],
            command: String::new(),
        })
        .await
        .is_err()
    {
        return;
    }

    let stream_req =
        tonic::Request::new(tokio_stream::wrappers::ReceiverStream::new(grpc_rx));

    let mut grpc_out = match agent.stream_console(stream_req).await {
        Ok(r) => r.into_inner(),
        Err(e) => {
            error!("console: stream_console failed for {vm_id}: {e}");
            return;
        }
    };

    let (ws_tx, ws_rx): (SplitSink<WebSocket, Message>, SplitStream<WebSocket>) = ws.split();

    // ws → gRPC
    let fwd = tokio::spawn(ws_to_grpc(ws_rx, grpc_tx));

    // gRPC → ws
    grpc_to_ws(&mut grpc_out, ws_tx).await;

    fwd.abort();
}

async fn ws_to_grpc(
    mut ws_rx: SplitStream<WebSocket>,
    grpc_tx: tokio::sync::mpsc::Sender<ConsoleInput>,
) {
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Binary(data) => {
                if grpc_tx
                    .send(ConsoleInput {
                        vm_id: String::new(),
                        data: data.to_vec(),
                        command: String::new(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn grpc_to_ws(
    grpc_out: &mut tonic::Streaming<agent_proto::agent::ConsoleOutput>,
    mut ws_tx: SplitSink<WebSocket, Message>,
) {
    while let Some(result) = grpc_out.next().await {
        match result {
            Ok(output) => {
                if ws_tx
                    .send(Message::Binary(output.data.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                error!("console: gRPC stream error: {e}");
                break;
            }
        }
    }
}
