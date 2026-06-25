# tunnel

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/rust-1.85%2B-orange)
[![Crates.io](https://img.shields.io/badge/crates.io-0.1.0-blue)](https://crates.io/crates/tunnel-client)

**Self-hosted ngrok alternative in Rust.** Expose localhost behind NAT or a firewall to the public internet through a TLS-encrypted tunnel. Zero ongoing cost when deployed on free-tier infrastructure.

```bash
# start tunnel server on a VM
tunnel-server --token s3cret --domain tunnel.example.com

# expose localhost:3000 from your dev machine
tunnel http 3000 --server tunnel.example.com:9000 --token s3cret
```

Tunnel URLs work in any browser. No client-side software needed for visitors.

---

## Features

- **Auth tokens** — tunnels require a shared secret; invalid tokens rejected at registration
- **TLS everywhere** — wire traffic between client and server is encrypted with TLS
- **Self-signed certs** — automatic dev certs; `--insecure` (`-k`) flag on the client to accept them
- **Custom subdomains** — `tunnel http 3000 -d myapp` → `https://myapp.example.com`
- **Stream multiplexing** — single persistent TLS connection carries many concurrent HTTP requests
- **Heartbeat keep-alive** — idle connection detection and cleanup
- **Visitor dashboard** — web UI at `/_tunnel/dashboard` listing active tunnels
- **Automatic URL-safe subdomains** — random 8-char hex subdomain when not specified

---

## How it works

```
┌──────────────┐     TLS tunnel      ┌────────────────┐    HTTP/HTTPS     ┌──────────┐
│  Your machine │ ──────────────────▶ │  tunnel-server  │ ◀──────────────── │ Visitor  │
│  tunnel-client │    :9000 (mux)     │  (free VM)      │     :443         │  Browser  │
│  localhost:3000│                   │  tunnel.example  │                  │          │
└──────────────┘                     └────────────────┘                  └──────────┘
```

1. **tunnel-client** opens a single TLS connection to **tunnel-server** and registers a subdomain.
2. **tunnel-server** listens for HTTP visitors on port 443. On each request, it serializes the request into a `HttpRequest` frame and sends it over the tunnel.
3. **tunnel-client** receives the frame, forwards it to `localhost:<port>`, reads the response, and sends back a `HttpResponse` frame.
4. **tunnel-server** relays the response to the visitor.

All concurrent requests are multiplexed over one TCP connection using stream IDs.

---

## Installation

### Cargo

```bash
cargo install tunnel-client tunnel-server
```

### Binary releases

Download pre-built binaries from the [releases page](https://github.com/shamirul-007/tunnel/releases) (Linux x86_64, macOS x86_64/ARM, optional Windows).

### Docker

```bash
# server
docker run -d -p 443:443 -p 9000:9000 --name tunnel-server \
  -e TUNNEL_TOKEN=s3cret -e TUNNEL_DOMAIN=example.com \
  tunnel:latest

# client
docker run --rm tunnel http 3000 \
  --server tunnel.example.com:9000 --token s3cret
```

### Build from source

```bash
git clone https://github.com/shamirul-007/tunnel.git
cd tunnel
cargo build --release
# binaries at target/release/tunnel-{server,client}
```

---

## Quick start

### Development (localhost, self-signed certs)

```bash
# terminal 1 — start the server
TUNNEL_TOKEN=devtoken TUNNEL_DOMAIN=localhost tunnel-server

# terminal 2 — start something to expose
python3 -m http.server 3000

# terminal 3 — expose it
tunnel http 3000 --server localhost:9000 --token devtoken
```

Open `https://dev-<hash>.localhost:443` in your browser (accept the self-signed TLS warning).

### Production (real domain, Let's Encrypt)

1. Point `tunnel.example.com` DNS to your server.
2. Get TLS certificates (e.g., via [acme.sh](https://github.com/acmesh-official/acme.sh) or Caddy).
3. Start the server:

```bash
tunnel-server \
  --token s3cret \
  --domain tunnel.example.com \
  --cert-file /etc/letsencrypt/live/tunnel.example.com/fullchain.pem \
  --key-file /etc/letsencrypt/live/tunnel.example.com/privkey.pem
```

4. Expose your local service:

```bash
tunnel http 3000 \
  --server tunnel.example.com:9000 \
  --token s3cret \
  -d myapp
```

Your tunnel is live at `https://myapp.tunnel.example.com`.

---

## Usage

### HTTP tunnel

```bash
tunnel http <port> [options]
```

| Flag | Short | Env | Default | Description |
|------|-------|-----|---------|-------------|
| `--subdomain` | `-d` | — | random 8-char hex | Requested subdomain |
| `--server` | `-s` | — | `localhost:9000` | Address of tunnel-server |
| `--token` | `-t` | `TUNNEL_TOKEN` | — | Auth token |
| `--insecure` | `-k` | — | `false` | Accept self-signed TLS certs |
| `--config` | `-c` | — | — | Config file path (optional) |

**Examples:**

```bash
# expose port 3000 with random subdomain
tunnel http 3000 -s tunnel.example.com:9000 -t s3cret

# expose with custom subdomain
tunnel http 8080 -d api -s tunnel.example.com:9000 -t s3cret

# local dev with self-signed certs
tunnel http 3000 -k
```

### TCP tunnel (experimental)

```bash
tunnel tcp <port> [options]
```

Currently implemented as an HTTP tunnel with a TCP-prefixed subdomain. Full raw TCP framing is in development.

### Server

```bash
tunnel-server [options]
```

| Flag | Env | Default | Description |
|------|-----|---------|-------------|
| `--bind` | — | `0.0.0.0` | Bind address |
| `--tunnel-port` | — | `9000` | Port for tunnel client connections |
| `--http-port` | — | `443` | Port for visitor HTTP/HTTPS traffic |
| `--http-redirect-port` | — | `80` | Redirect HTTP→HTTPS (set `0` to disable) |
| `--domain` | `TUNNEL_DOMAIN` | `localhost` | Public domain for tunnel URLs |
| `--token` | `TUNNEL_TOKEN` | — | Auth token (required) |
| `--cert-file` | — | — | TLS cert file path |
| `--key-file` | — | — | TLS key file path |

---

## Deployment

### fly.io

```bash
fly launch --name my-tunnel
fly secrets set TUNNEL_TOKEN=s3cret TUNNEL_DOMAIN=my-tunnel.fly.dev
fly deploy
```

fly.io handles TLS termination and provides a free `*.fly.dev` subdomain, so `--cert-file` and `--key-file` are not needed.

### Docker

```bash
docker build -t tunnel-server .
docker run -d --restart unless-stopped \
  -p 443:443 -p 9000:9000 \
  -v /etc/letsencrypt:/etc/letsencrypt:ro \
  tunnel-server \
  --token s3cret --domain tunnel.example.com \
  --cert-file /etc/letsencrypt/live/tunnel.example.com/fullchain.pem \
  --key-file /etc/letsencrypt/live/tunnel.example.com/privkey.pem
```

### Reverse proxy (Caddy / Nginx)

You can place Caddy or Nginx in front of the HTTP listener for automatic TLS:

```caddy
tunnel.example.com {
    reverse_proxy localhost:443
}
```

Then start tunnel-server with `--http-port 8080` and let Caddy handle port 443.

---

## Wire protocol

Versioned binary framing over TLS:

```
┌─────────┬──────────────┬────────┬──────────────────┐
│ version │  stream_id   │  type  │  payload_len      │
│  u8     │  u32 BE      │  u8    │  u32 BE           │
├─────────┼──────────────┼────────┼──────────────────┤
│   1     │  0x00000001  │  0x03  │  0x0000009A       │
└─────────┴──────────────┴────────┴──────────────────┘
┌───────────────────────────────────────┐
│             payload                    │
│             (JSON, N bytes)            │
└───────────────────────────────────────┘
```

**Message types:**

| Type | Name | Direction | Payload |
|------|------|-----------|---------|
| 0x01 | Register | Client → Server | `{subdomain, local_port, token}` |
| 0x02 | Registered | Server → Client | `{assigned_url, tunnel_id}` |
| 0x03 | HttpRequest | Server → Client | `{method, uri, headers, visitor}` |
| 0x04 | HttpResponse | Client → Server | `{status, headers, body}` |
| 0x05 | TcpData | Bidirectional | Raw bytes (future use) |
| 0x06 | Error | Bidirectional | `{message}` |
| 0x07 | CloseStream | Bidirectional | — |
| 0x08 | Heartbeat | Bidirectional | — |

---

## Authentication

Tunnel-server requires a `--token` that every client must present in its `Register` frame. Tokens are compared on the server before the tunnel is established. Clients pass the token via:

- `--token` flag
- `TUNNEL_TOKEN` environment variable

Tokens are transmitted inside the TLS-encrypted tunnel, so they are never exposed on the wire.

---

## Why Rust?

| | **tunnel** | **ngrok** | **frp** | **bore** |
|---|---|---|---|---|
| Language | Rust | Go | Go | Rust |
| Memory safety | ✅ compile-time | ✅ (GC) | ✅ (GC) | ✅ compile-time |
| Binary size | ~5 MB | ~15 MB | ~10 MB | ~3 MB |
| Startup time | instant | instant | instant | instant |
| Community vibe | 🦀 new tooling | established | established | minimal |
| Protocol | custom binary | HTTP/2 | custom TCP | custom TCP |

tunnel was written in Rust because all major tunnel tools are Go. Rust gives memory safety without a GC, tiny binaries, and a community eager for new infrastructure tooling.

---

## Development

### Build

```bash
cargo build
```

### Test

```bash
cargo test
```

### Run full integration

```bash
# terminal 1 — server
TUNNEL_TOKEN=test123 TUNNEL_DOMAIN=localhost cargo run --bin tunnel-server

# terminal 2 — test server
python3 -m http.server 3000

# terminal 3 — tunnel client
cargo run --bin tunnel-client http 3000 -s localhost:9000 -d test -k
```

### Project structure

```
tunnel/
├── tunnel-proto/        # wire protocol: Frame types, Codec (encode/decode)
├── tunnel-server/       # axum-based HTTP server + TLS tunnel listener
│   ├── main.rs          # CLI args, startup orchestration
│   ├── tunnel.rs        # TunnelManager, stream multiplexing, proxying
│   └── tls.rs           # TlsConfig, self-signed cert generation
├── tunnel-client/       # CLI client
│   ├── main.rs          # CLI args (http/tcp subcommands)
│   └── tunnel.rs        # TLS connect, NoCertVerifier, request forwarding
└── Cargo.toml           # workspace root
```

### Contributing

1. Fork the repo
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit your changes (`git commit -am 'add my feature'`)
4. Push to the branch (`git push origin feat/my-feature`)
5. Open a Pull Request

Please ensure `cargo test` passes and `cargo clippy` is clean.

---

## Roadmap

- [ ] Raw TCP tunnel (SSH, databases, custom protocols)
- [ ] WebSocket passthrough (HMR, live-reload)
- [ ] Client configuration file (`~/.tunnel/config.toml`)
- [ ] Prometheus metrics endpoint
- [ ] Tunnel management API (list, close tunnels programmatically)
- [ ] Homebrew formula + Scoop bucket
- [ ] GitHub Actions CI + cross-compilation releases
- [ ] Docker image on GHCR

---

## License

MIT — see [LICENSE](LICENSE) for details.

---

*Inspired by [bore](https://github.com/ekzhang/bore), [frp](https://github.com/fatedier/frp), and the Rust community's love for infrastructure tooling.*
