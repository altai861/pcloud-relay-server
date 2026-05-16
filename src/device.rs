use crate::{
    protocol::DeviceToRelay,
    state::{AppState, DeviceConnection, PendingResponse},
};
use axum::{
    Json,
    body::Bytes,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

#[derive(Debug, Deserialize)]
pub struct DeviceConnectQuery {
    device_id: String,
    token: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceStatusResponse {
    device_id: String,
    online: bool,
}

pub async fn device_status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Json<DeviceStatusResponse> {
    let online = state.devices.read().await.contains_key(&device_id);

    Json(DeviceStatusResponse { device_id, online })
}

pub async fn device_connect(
    State(state): State<AppState>,
    Query(query): Query<DeviceConnectQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if query.token != *state.shared_token {
        return (StatusCode::UNAUTHORIZED, "Invalid relay token").into_response();
    }

    ws.on_upgrade(move |socket| handle_device_socket(state, query.device_id, socket))
}

async fn handle_device_socket(state: AppState, device_id: String, socket: WebSocket) {
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let (outgoing_sender, mut outgoing_receiver) = mpsc::channel::<Message>(128);

    let connection = Arc::new(DeviceConnection {
        sender: outgoing_sender,
        pending: Mutex::new(Default::default()),
    });

    state
        .devices
        .write()
        .await
        .insert(device_id.clone(), connection.clone());

    println!("device connected: {device_id}");

    let writer_device_id = device_id.clone();
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outgoing_receiver.recv().await {
            if socket_sender.send(message).await.is_err() {
                break;
            }
        }
        println!("device writer stopped: {writer_device_id}");
    });

    while let Some(message_result) = socket_receiver.next().await {
        match message_result {
            Ok(Message::Text(text)) => handle_device_text(&connection, &text).await,
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Binary(_)) => {}
            Err(error) => {
                eprintln!("device socket error for {device_id}: {error}");
                break;
            }
        }
    }

    writer_task.abort();

    remove_device_if_same_connection(&state, &device_id, &connection).await;
    fail_pending_requests(&connection).await;

    println!("device disconnected: {device_id}");
}

async fn handle_device_text(connection: &Arc<DeviceConnection>, text: &str) {
    let message = match serde_json::from_str::<DeviceToRelay>(text) {
        Ok(message) => message,
        Err(error) => {
            eprintln!("invalid device message: {error}");
            return;
        }
    };

    match message {
        DeviceToRelay::ResponseStart(response) => {
            let request_id = response.request_id.clone();
            let start_sender = {
                let mut pending = connection.pending.lock().await;
                pending
                    .get_mut(&request_id)
                    .and_then(|pending| pending.start_sender.take())
            };

            if let Some(sender) = start_sender {
                let _ = sender.send(response);
            }
        }
        DeviceToRelay::ResponseBodyChunk(chunk) => {
            let body_sender = {
                let pending = connection.pending.lock().await;
                pending
                    .get(&chunk.request_id)
                    .map(|pending| pending.body_sender.clone())
            };

            let Some(body_sender) = body_sender else {
                return;
            };

            match decode_body(&chunk.body_base64) {
                Ok(body) => {
                    let _ = body_sender.send(Ok(Bytes::from(body))).await;
                }
                Err(error) => {
                    let _ = body_sender
                        .send(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Invalid response body chunk: {error}"),
                        )))
                        .await;
                }
            }
        }
        DeviceToRelay::ResponseEnd(end) => {
            connection.pending.lock().await.remove(&end.request_id);
        }
        DeviceToRelay::ResponseError(error) => {
            let pending = connection.pending.lock().await.remove(&error.request_id);
            if let Some(PendingResponse {
                start_sender,
                body_sender,
            }) = pending
            {
                let message = error.message;
                if let Some(start_sender) = start_sender {
                    let _ = start_sender.send(crate::protocol::RelayResponseStart {
                        request_id: error.request_id,
                        status: error.status,
                        headers: vec![crate::protocol::HeaderPair {
                            name: "content-type".to_owned(),
                            value: "text/plain; charset=utf-8".to_owned(),
                        }],
                    });
                    let _ = body_sender.send(Ok(Bytes::from(message))).await;
                } else {
                    let _ = body_sender.send(Err(io::Error::other(message))).await;
                }
            }
        }
        DeviceToRelay::Pong => {}
    }
}

fn decode_body(body_base64: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{Engine as _, engine::general_purpose};

    general_purpose::STANDARD.decode(body_base64)
}

async fn remove_device_if_same_connection(
    state: &AppState,
    device_id: &str,
    connection: &Arc<DeviceConnection>,
) {
    let mut devices = state.devices.write().await;
    let should_remove = devices
        .get(device_id)
        .map(|current| Arc::ptr_eq(current, connection))
        .unwrap_or(false);

    if should_remove {
        devices.remove(device_id);
    }
}

async fn fail_pending_requests(connection: &Arc<DeviceConnection>) {
    let mut pending = connection.pending.lock().await;
    pending.clear();
}
