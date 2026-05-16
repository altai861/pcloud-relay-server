# pCloud Relay Server

Public relay service for pCloud personal storage devices.

This service is intentionally separate from the main pCloud server app. The main
server runs on the user's device, while this relay runs on a public VPS/cloud
host and forwards remote browser/mobile traffic through an outbound tunnel kept
open by the device.

## Architecture

```text
Browser / iOS app
      |
      | HTTPS
      v
pCloud Relay Server
      |
      | WebSocket tunnel opened by device
      v
User pCloud Server on Raspberry Pi / home device
```

The relay does not store user files. HTTP metadata is sent as JSON messages over
the device WebSocket, while request and response bodies are forwarded as bounded
base64 chunks. Chunks are currently capped at 64 KiB, with bounded per-request
queues, so the relay avoids buffering full uploads/downloads in memory.

## Environment

```bash
PCLOUD_RELAY_BIND=0.0.0.0:7070
PCLOUD_RELAY_TOKEN=change-this-shared-development-token
PCLOUD_RELAY_REQUEST_TIMEOUT_SECONDS=3600
PCLOUD_RELAY_MAX_BODY_BYTES=5368709120
```

`PCLOUD_RELAY_TOKEN` is required. The current MVP uses one shared token for
device tunnel authentication. A production version should replace this with
per-device credentials or signed device keys.

## Run

```bash
cargo run
```

Health check:

```bash
curl http://localhost:7070/health
```

Device status:

```bash
curl http://localhost:7070/api/devices/my-device/status
```

## Device Tunnel Endpoint

The user's pCloud server should open a WebSocket connection to:

```text
ws://relay-host:7070/api/relay/device/connect?device_id=my-device&token=...
```

After connection, the relay can proxy browser/mobile requests to:

```text
http://relay-host:7070/d/my-device/
http://relay-host:7070/d/my-device/api/client/status
http://relay-host:7070/d/my-device/api/client/storage/list
```

Everything after `/d/{device_id}` is forwarded to the device server as if it
were requested locally from `/`.

## Protocol

Relay starts a proxied HTTP request with metadata:

```json
{
  "type": "request_start",
  "request_id": "1",
  "method": "GET",
  "path": "/api/client/status",
  "headers": []
}
```

Request body chunks are sent separately:

```json
{
  "type": "request_body_chunk",
  "request_id": "1",
  "body_base64": "..."
}
```

The relay finishes the request body with:

```json
{
  "type": "request_end",
  "request_id": "1"
}
```

The device responds with metadata first:

```json
{
  "type": "response_start",
  "request_id": "1",
  "status": 200,
  "headers": [
    {
      "name": "content-type",
      "value": "application/json"
    }
  ]
}
```

Response body chunks are streamed back:

```json
{
  "type": "response_body_chunk",
  "request_id": "1",
  "body_base64": "eyJzdGF0dXMiOiJvayJ9"
}
```

The device finishes the response body with:

```json
{
  "type": "response_end",
  "request_id": "1"
}
```

## Main Server Relay Client

The main pCloud server has an optional relay client. Enable it with:

```bash
PCLOUD_RELAY_ENABLED=true
PCLOUD_RELAY_URL=ws://127.0.0.1:7070/api/relay/device/connect
PCLOUD_DEVICE_ID=my-device
PCLOUD_RELAY_TOKEN=change-this-shared-development-token
PCLOUD_RELAY_LOCAL_BASE_URL=http://127.0.0.1:8080
```

When enabled, the main server connects to this relay, receives streamed request
messages, forwards them to its local web/API server, and streams response
messages back through the tunnel.

Later the local forwarding can dispatch directly into the Axum service instead
of calling `http://127.0.0.1:8080`.
