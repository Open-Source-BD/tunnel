<p align="center">
  <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT">
  <img src="https://img.shields.io/badge/rust-1.85%2B-orange" alt="Rust">
  <img src="https://img.shields.io/badge/status-beta-yellow" alt="Beta">
  <img src="https://img.shields.io/badge/crates.io-0.1.0-blue" alt="crates.io">
</p>

<h1 align="center">tunnel</h1>

<p align="center">
  <strong>Expose localhost to the internet — one command, zero cost.</strong><br>
  Self-hosted tunnel server in Rust. TLS encrypted. Auth protected. Works in any browser.
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> •
  <a href="#installation">Installation</a> •
  <a href="#usage">Usage</a> •
  <a href="#deployment">Deployment</a> •
  <a href="#wire-protocol">Protocol</a> •
  <a href="#development">Development</a>
</p>

---

```bash
# start server on a VM
tunnel-server --token s3cret --domain tunnel.example.com

# expose localhost:3000
tunnel http 3000 --server tunnel.example.com:9000 --token s3cret
```

Works for HTTP APIs, web apps, static sites, and anything that speaks HTTP. Visitors open `https://<subdomain>.tunnel.example.com` in any browser — no client-side install needed.

---

## Features

- **TLS by default** — all traffic between client and server is encrypted
- **Token auth** — only authorized clients can register tunnels
- **Custom subdomains** — `tunnel http 3000 -d myapp` → `https://myapp.example.com`
- **Self-signed support** — automatic dev certs, `-k` to accept on the client
- **Stream multiplexing** — one TLS connection handles many concurrent requests
- **Heartbeat keep-alive** — idle detection and cleanup
- **Visitor dashboard** — live tunnel list at `/_tunnel/dashboard`
- **80→443 redirect** — built-in HTTP→HTTPS upgrade

---

## Table of Contents

- [Quick Start](#quick-start)
- [Installation](#installation)
- [Usage](#usage)
- [Deployment](#deployment)
- [Architecture](#architecture)
- [Wire Protocol](#wire-protocol)
- [Authentication](#authentication)
- [Why Rust?](#why-rust)
- [Development](#development)
- [Contributing](#contributing)
- [Roadmap](#roadmap)
- [License](#license)

---

## Quick Start

### Local dev (self-signed certs)

```bash
# terminal 1 — start server
TUNNEL_TOKEN=devtoken TUNNEL_DOMAIN=localhost tunnel-server

# terminal 2 — start something to expose
python3 -m http.server 3000

# terminal 3 — expose it
tunnel http 3000 --server localhost:9000 --token devtoken -k
```

Open `https://dev-a1b2c3d4.localhost:443` in your browser (accept self-signed warning).

### Production (real domain, Let's Encrypt)

```bash
# on your VM
tunnel-server \
  --token s3cret \
  --domain tunnel.example.com \
  --cert-file /etc/letsencrypt/live/tunnel.example.com/fullchain.pem \
  --key-file /etc/letsencrypt/live/tunnel.example.com/privkey.pem

# on your dev machine
tunnel http 3000 -s tunnel.example.com:9000 -t s3cret -d myapp
```

Your tunnel: **`https://myapp.tunnel.example.com`**

---

## Installation

### From source

Requires Rust 1.85+.

```bash
git clone https://github.com/shamirul-007/tunnel.git
cd tunnel
cargo build --release
# binaries at target/release/tunnel-{server,client}
```

### Cargo install

```bash
cargo install tunnel-client tunnel-server
```

### Docker

```bash
# server
docker run -d -p 443:443 -p 9000:9000 \
  -e TUNNEL_TOKEN=s3cret -e TUNNEL_DOMAIN=example.com \
  tunnel-server

# client
docker run --rm tunnel http 3000 \
  -s tunnel.example.com:9000 -t s3cret
```

### Binary releases

Pre-built binaries for Linux, macOS, and Windows are available on the [releases page](https://github.com/shamirul-007/tunnel/releases).

---

## Usage

### HTTP tunnel

```bash
tunnel http <port> [options]
```

| Option | Short | Env | Default | Description |
|--------|-------|-----|---------|-------------|
| `--subdomain` | `-d` | — | random | Requested subdomain name |
| `--server` | `-s` | — | `localhost:9000` | Tunnel server address |
| `--token` | `-t` | `TUNNEL_TOKEN` | — | Auth token (required) |
| `--insecure` | `-k` | — | `false` | Accept self-signed TLS certs |
| `--config` | `-c` | — | — | Config file path |

Examples:

```bash
tunnel http 3000 -s tunnel.example.com:9000 -t s3cret     # random subdomain
tunnel http 3000 -d api -s tunnel.example.com:9000 -t s3cret  # custom subdomain
tunnel http 3000 -k                                       # local dev, self-signed
```

### TCP tunnel (experimental)

```bash
tunnel tcp <port> [options]
```

Currently wraps TCP connections as HTTP tunnels with a `tcp-` prefixed subdomain. Full raw TCP framing is in development.

### Server

```bash
tunnel-server [options]
```

| Option | Env | Default | Description |
|--------|-----|---------|-------------|
| `--bind` | — | `0.0.0.0` | Bind address |
| `--tunnel-port` | — | `9000` | Port for tunnel client connections |
| `--http-port` | — | `443` | Port for visitor HTTP traffic |
| `--http-redirect-port` | — | `80` | HTTP→HTTPS redirect (0 to disable) |
| `--domain` | `TUNNEL_DOMAIN` | `localhost` | Public domain for tunnel URLs |
| `--token` | `TUNNEL_TOKEN` | — | Auth token (required) |
| `--cert-file` | — | — | TLS certificate path |
| `--key-file` | — | — | TLS key path |

### Environment variables

| Variable | Used by | Description |
|----------|---------|-------------|
| `TUNNEL_TOKEN` | client, server | Auth token |
| `TUNNEL_DOMAIN` | server | Public domain |

---

## Deployment

### fly.io (free tier)

```bash
fly launch --name my-tunnel
fly secrets set TUNNEL_TOKEN=s3cret TUNNEL_DOMAIN=my-tunnel.fly.dev
fly deploy
```

fly.io provides free TLS termination and a `*.fly.dev` subdomain — no cert files needed.

### Docker

```bash
docker build -t tunnel-server .
docker run -d --restart unless-stopped \
  -p 443:443 -p 9000:9000 \
  tunnel-server \
  --token s3cret --domain tunnel.example.com
```

### Reverse proxy

Place Caddy or Nginx in front of the HTTP listener for automatic TLS:

```caddyfile
tunnel.example.com {
    reverse_proxy localhost:8080
}
```

Then start the server with `--http-port 8080` and let Caddy handle 443.

---

## Architecture

```
┌──────────────┐     1 TLS connection       ┌──────────────┐     HTTP      ┌──────────┐
│  tunnel-client │ ────── multiplexed ──────▶ │ tunnel-server │ ◀─────────── │ Visitor  │
│  (dev machine) │         frames            │  (free VM)   │    :443      │  Browser  │
└──────────────┘                             └──────────────┘              └──────────┘
```

The client opens **one** persistent TLS connection to the server. All visitor requests are serialized into binary frames and multiplexed over this single connection using stream IDs. This avoids NAT/firewall issues and keeps the connection overhead minimal.

**Request flow:**

1. Visitor hits `https://myapp.example.com/path`
2. Server extracts `myapp` subdomain from the `Host` header
3. Server assigns a stream ID, serializes the request as an `HttpRequest` frame
4. Frame is sent to the matching tunnel client
5. Client forwards the request to `localhost:<port>`
6. Client reads the response, sends it back as an `HttpResponse` frame
7. Server relays the response to the visitor

---

## Wire Protocol

Binary framing over TLS:

```
┌─────────┬──────────────┬────────┬──────────────────┐
│ version │  stream_id   │  type  │  payload_len     │
│   u8    │   u32 BE     │   u8   │   u32 BE         │
├─────────┼──────────────┼────────┼──────────────────┤
│   1     │  0x00000001  │  0x03  │  0x0000009A      │
└─────────┴──────────────┴────────┴──────────────────┘
┌───────────────────────────────────────┐
│           payload (JSON)              │
│           N bytes                     │
└───────────────────────────────────────┘
```

**Message types:**

| Type | Frame | Direction | Payload |
|------|-------|-----------|---------|
| `0x01` | Register | Client → Server | `{subdomain, local_port, token}` |
| `0x02` | Registered | Server → Client | `{assigned_url, tunnel_id}` |
| `0x03` | HttpRequest | Server → Client | `{method, uri, headers, visitor}` |
| `0x04` | HttpResponse | Client → Server | `{status, headers, body}` |
| `0x05` | TcpData | Bidirectional | raw bytes |
| `0x06` | Error | Bidirectional | `{message}` |
| `0x07` | CloseStream | Bidirectional | — |
| `0x08` | Heartbeat | Bidirectional | — |

The version field (`0x01`) ensures forward compatibility. A future v2 could switch to HTTP/2 multiplexing while old clients still connect with v1.

---

## Authentication

The server requires a `--token` on startup. Every client must present the same token in its `Register` frame or the connection is rejected with an `Error` frame and dropped. Tokens are transmitted inside the TLS tunnel and never exposed on the wire.

Clients can pass the token via `--token` flag or `TUNNEL_TOKEN` environment variable.

---

## Why Rust?

All major tunnel tools (ngrok, frp, bore) are Go. Rust gives us:

- **Memory safety** without a garbage collector
- **Zero-cost abstractions** — no runtime overhead
- **Small binaries** (~5 MB stripped)
- **Sub-millisecond startup** — no VM warmup
- **Ecosystem fit** — tokio, axum, rustls are the best-in-class async stack

| Tool | Language | Binary | TLS | Auth |
|------|----------|--------|-----|------|
| **tunnel** | Rust | ~5 MB | ✅ built-in | ✅ token |
| ngrok | Go | ~15 MB | ✅ built-in | ✅ account required |
| frp | Go | ~10 MB | ✅ built-in | ✅ token/OIDC |
| bore | Rust | ~3 MB | ❌ (tunnel only) | ✅ HMAC |

---

## Development

```bash
# build all crates
cargo build

# run tests
cargo test

# run with live output
TUNNEL_TOKEN=test123 TUNNEL_DOMAIN=localhost cargo run --bin tunnel-server

# in another terminal
cargo run --bin tunnel-client http 3000 -s localhost:9000 -d test -k
```

### Project structure

```
tunnel/
├── Cargo.toml              # workspace root
├── tunnel-proto/           # wire protocol: Frame types, Codec
│   ├── src/types.rs        #   message types, payload structs
│   └── src/codec.rs        #   async encode/decode framing
├── tunnel-server/          # axum HTTP server + TLS listener
│   ├── src/main.rs         #   CLI args, startup orchestration
│   ├── src/tunnel.rs       #   TunnelManager, multiplexing, proxying
│   └── src/tls.rs          #   TlsConfig, self-signed cert generation
└── tunnel-client/          # CLI tunnel client
    ├── src/main.rs         #   CLI args (http/tcp subcommands)
    └── src/tunnel.rs       #   TLS connect, NoCertVerifier, forwarding
```

---

## Roadmap

- [ ] Raw TCP tunnel (SSH, databases, custom protocols)
- [ ] WebSocket passthrough
- [ ] Client config file (`~/.tunnel/config.toml`)
- [ ] Prometheus metrics
- [ ] Tunnel management API
- [ ] Homebrew formula
- [ ] CI + cross-compilation releases
- [ ] Docker image on GHCR

---

## Contributing

PRs welcome. Please ensure `cargo test` passes and `cargo clippy` is clean.

1. Fork the repo
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit your changes (`git commit -am 'add my feature'`)
4. Push (`git push origin feat/my-feature`)
5. Open a Pull Request

---

## License

MIT — see [LICENSE](LICENSE).

---

*Inspired by [bore](https://github.com/ekzhang/bore), [frp](https://github.com/fatedier/frp), and the Rust community.*
