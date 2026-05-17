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

Single-node mode only needs the shared device tunnel token:

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

## Docker VPS Deployment

The MVP deployment supports two Docker Compose modes:

- IP-only HTTP mode for quick testing without a domain.
- Domain HTTPS mode using Caddy when you are ready to add DNS.

### IP-only HTTP Mode

This mode runs only the relay and exposes it at `http://<VPS_IP>:7070`.

```bash
cp .env.ip.example .env
nano .env
docker compose -f docker-compose.ip.yml up -d --build
```

Set a strong token before starting:

```bash
PCLOUD_RELAY_TOKEN=<long-random-device-token>
```

Open port `7070` on the VPS firewall. The device should use:

```bash
PCLOUD_RELAY_URL=ws://<VPS_IP>:7070/api/relay/device/connect
```

Remote clients can access:

```text
http://<VPS_IP>:7070/d/<device_id>/
```

This is fine for an MVP demo, but traffic is plain HTTP/WebSocket. Do not reuse
important passwords over this connection.

### Domain HTTPS Mode

When you have a domain, use the default compose file. It runs:

- `relay`: the Rust relay server
- `caddy`: HTTPS reverse proxy with automatic TLS certificates

Cloudflare DNS setup:

1. In Cloudflare, open your domain's DNS records.
2. Add an `A` record:

```text
Type: A
Name: relay
IPv4 address: <VPS_PUBLIC_IP>
Proxy status: DNS only
TTL: Auto
```

This creates `relay.example.com`. Keep it as `DNS only` at first so Caddy can
issue the Let's Encrypt certificate directly. After HTTPS works, you may turn on
the Cloudflare proxy and set Cloudflare SSL/TLS mode to `Full (strict)`.

On a new Ubuntu VPS, install Docker before using either mode:

```bash
sudo apt-get update
sudo apt-get install -y ca-certificates curl
sudo install -m 0755 -d /etc/apt/keyrings
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
```

Then configure and start the relay:

```bash
cp .env.example .env
nano .env
docker compose up -d --build
```

Set these values before starting:

```bash
PCLOUD_RELAY_DOMAIN=relay.<your-domain>
PCLOUD_RELAY_TOKEN=<long-random-device-token>
```

Open firewall ports `80` and `443`. The default compose file binds relay port
`7070` only to `127.0.0.1`, so public traffic enters through HTTPS.

Check it:

```bash
curl https://relay.<your-domain>/health
```

The device should use:

```bash
PCLOUD_RELAY_URL=wss://relay.<your-domain>/api/relay/device/connect
```

Remote clients can access:

```text
https://relay.<your-domain>/d/<device_id>/
```

Useful operations:

```bash
docker compose ps
docker compose logs -f relay
docker compose logs -f caddy
docker compose pull
docker compose up -d --build
docker compose down
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

## Future Horizontal Scaling

The current MVP stores connected devices in memory on one relay process. That is
simple and enough for a single VPS deployment, but it means a device connected to
one relay process can only be reached through that process.

Horizontal scaling can be added later with one of these approaches:

1. Add sticky load balancing by `device_id`, so browser requests and the device
   WebSocket for the same device always reach the same relay node.
2. Add a shared device registry such as Redis, storing
   `device_id -> relay_node_internal_url`, then forward requests from non-owning
   relay nodes to the owning relay node.
3. Let each device open multiple relay tunnels to different nodes and distribute
   requests across those tunnels.

The recommended future design is option 2. Each relay node would register its
connected devices in Redis with a short TTL, refresh the TTL while the WebSocket
is alive, and remove the registration on disconnect. Public requests could land
on any relay node; the node would look up the owning relay and forward the HTTP
stream internally. This keeps the MVP protocol mostly unchanged while allowing
multiple VPS nodes behind a public load balancer.
