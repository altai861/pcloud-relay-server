use crate::{
    protocol::{
        BodyChunk, HeaderPair, RelayRequestStart, RelayResponseStart, RelayToDevice, RequestAbort,
        RequestEnd,
    },
    state::{AppState, PendingResponse},
};
use axum::{
    body::{Body, Bytes},
    extract::{OriginalUri, Path, State, ws::Message},
    http::{
        HeaderMap, HeaderName, HeaderValue, Request, StatusCode, Uri,
        header::{CONNECTION, CONTENT_LENGTH, HOST, LOCATION, TRANSFER_ENCODING, UPGRADE},
    },
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use std::io;

const STREAM_CHUNK_BYTES: usize = 64 * 1024;
const RESPONSE_BUFFER_CHUNKS: usize = 16;

pub async fn redirect_device_root(
    Path(device_id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let query = original_uri
        .query()
        .map(|value| format!("?{value}"))
        .unwrap_or_default();
    let location = format!("/d/{device_id}/{query}");

    (StatusCode::TEMPORARY_REDIRECT, [(LOCATION, location)]).into_response()
}

pub async fn proxy_root(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    request: Request<Body>,
) -> Response {
    proxy_to_device(state, device_id, original_uri, request).await
}

pub async fn proxy_path(
    State(state): State<AppState>,
    Path((device_id, _path)): Path<(String, String)>,
    OriginalUri(original_uri): OriginalUri,
    request: Request<Body>,
) -> Response {
    proxy_to_device(state, device_id, original_uri, request).await
}

async fn proxy_to_device(
    state: AppState,
    device_id: String,
    original_uri: Uri,
    request: Request<Body>,
) -> Response {
    let connection = match state.devices.read().await.get(&device_id).cloned() {
        Some(connection) => connection,
        None => return (StatusCode::BAD_GATEWAY, "Device is offline").into_response(),
    };

    let request_id = state.next_request_id();
    let method = request.method().clone();
    let path = proxied_path(&device_id, &original_uri);
    let headers = relay_headers(request.headers());
    println!(
        "proxy request {request_id}: {} {} -> device {device_id} path {path}",
        method, original_uri
    );

    let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
    let (body_sender, body_receiver) = tokio::sync::mpsc::channel(RESPONSE_BUFFER_CHUNKS);
    connection.pending.lock().await.insert(
        request_id.clone(),
        PendingResponse {
            start_sender: Some(response_sender),
            body_sender,
        },
    );

    let relay_request = RelayToDevice::RequestStart(RelayRequestStart {
        request_id: request_id.clone(),
        method: method.as_str().to_owned(),
        path,
        headers,
    });

    if send_relay_message(&connection.sender, relay_request)
        .await
        .is_err()
    {
        connection.pending.lock().await.remove(&request_id);
        return (StatusCode::BAD_GATEWAY, "Device tunnel is closed").into_response();
    }

    let request_body = request.into_body();
    let body_forward_connection = connection.clone();
    let body_forward_request_id = request_id.clone();
    let max_body_bytes = state.max_body_bytes;
    tokio::spawn(async move {
        if let Err(error) = forward_request_body(
            body_forward_connection.sender.clone(),
            body_forward_request_id.clone(),
            request_body,
            max_body_bytes,
        )
        .await
        {
            let _ = send_relay_message(
                &body_forward_connection.sender,
                RelayToDevice::RequestAbort(RequestAbort {
                    request_id: body_forward_request_id,
                    message: error.to_string(),
                }),
            )
            .await;
        }
    });

    match tokio::time::timeout(state.request_timeout, response_receiver).await {
        Ok(Ok(relay_response_start)) => {
            println!(
                "proxy response {request_id}: device {device_id} returned {}",
                relay_response_start.status
            );
            build_proxy_response(relay_response_start, body_receiver)
        }
        Ok(Err(_)) => (
            StatusCode::BAD_GATEWAY,
            "Device tunnel closed before response",
        )
            .into_response(),
        Err(_) => {
            connection.pending.lock().await.remove(&request_id);
            (StatusCode::GATEWAY_TIMEOUT, "Device response timed out").into_response()
        }
    }
}

async fn forward_request_body(
    sender: tokio::sync::mpsc::Sender<Message>,
    request_id: String,
    mut body: Body,
    max_body_bytes: usize,
) -> anyhow::Result<()> {
    let mut total_bytes = 0usize;

    while let Some(frame_result) = body.frame().await {
        let frame = frame_result?;
        let Some(data) = frame.data_ref() else {
            continue;
        };

        total_bytes = total_bytes.saturating_add(data.len());
        if total_bytes > max_body_bytes {
            anyhow::bail!("Request body exceeded relay limit of {max_body_bytes} bytes");
        }

        for chunk in data.chunks(STREAM_CHUNK_BYTES) {
            send_relay_message(
                &sender,
                RelayToDevice::RequestBodyChunk(BodyChunk {
                    request_id: request_id.clone(),
                    body_base64: encode_body(chunk),
                }),
            )
            .await?;
        }
    }

    send_relay_message(
        &sender,
        RelayToDevice::RequestEnd(RequestEnd { request_id }),
    )
    .await?;

    Ok(())
}

async fn send_relay_message(
    sender: &tokio::sync::mpsc::Sender<Message>,
    message: RelayToDevice,
) -> anyhow::Result<()> {
    let serialized = serde_json::to_string(&message)?;
    sender
        .send(Message::Text(serialized))
        .await
        .map_err(|_| anyhow::anyhow!("Device tunnel is closed"))
}

fn proxied_path(device_id: &str, original_uri: &Uri) -> String {
    let original = original_uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");

    let prefix = format!("/d/{device_id}");
    let stripped = original.strip_prefix(&prefix).unwrap_or(original);

    if stripped.is_empty() {
        "/".to_owned()
    } else {
        stripped.to_owned()
    }
}

fn relay_headers(headers: &HeaderMap) -> Vec<HeaderPair> {
    headers
        .iter()
        .filter(|(name, _)| should_forward_header(name))
        .filter_map(|(name, value)| {
            Some(HeaderPair {
                name: name.as_str().to_owned(),
                value: value.to_str().ok()?.to_owned(),
            })
        })
        .collect()
}

fn should_forward_header(name: &HeaderName) -> bool {
    !matches!(
        name,
        &HOST | &CONNECTION | &UPGRADE | &TRANSFER_ENCODING | &CONTENT_LENGTH
    )
}

fn encode_body(body: &[u8]) -> String {
    general_purpose::STANDARD.encode(body)
}

fn build_proxy_response(
    relay_response: RelayResponseStart,
    body_receiver: tokio::sync::mpsc::Receiver<Result<Bytes, io::Error>>,
) -> Response {
    let status = StatusCode::from_u16(relay_response.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let stream = futures_util::stream::unfold(body_receiver, |mut receiver| async {
        receiver.recv().await.map(|item| (item, receiver))
    })
    .map(|item| {
        item.map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })
    });

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;

    for header in relay_response.headers {
        let Ok(name) = HeaderName::from_bytes(header.name.as_bytes()) else {
            continue;
        };
        if !should_forward_header(&name) {
            continue;
        }
        let Ok(value) = HeaderValue::from_str(&header.value) else {
            continue;
        };
        response.headers_mut().append(name, value);
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxied_path_strips_device_prefix() {
        let uri = "/d/device-1/api/files?limit=20".parse::<Uri>().unwrap();

        assert_eq!(proxied_path("device-1", &uri), "/api/files?limit=20");
    }

    #[test]
    fn proxied_path_maps_device_root_to_slash() {
        let uri = "/d/device-1".parse::<Uri>().unwrap();

        assert_eq!(proxied_path("device-1", &uri), "/");
    }
}
