# tunnel — free ngrok alternative

Self-host ngrok alternative in Rust. Expose localhost via public URLs. Free forever.

```bash
# Start the server (on a VM with public IP)
tunnel-server --token mysecret --domain myapp.duckdns.org

# Expose localhost:3000 (on your dev machine)
tunnel http 3000 --server myapp.duckdns.org:9000 --token mysecret
# Tunnel URL: https://dev-a1b2c3.myapp.duckdns.org
```

## Architecture

```
Client (your machine)  ──TLS──►  Server (free VM)  ◄──HTTP──  Visitor
  tunnel http 3000               tunnel-server                 Browser
  localhost:3000                  myapp.duckdns.org:443
```

Client opens 1 persistent TLS connection. Server multiplexes visitor requests over it.

## Quick start (dev)

```bash
# Run server with self-signed cert
TUNNEL_TOKEN=devtoken TUNNEL_DOMAIN=localhost tunnel-server

# In another terminal, start a local server
python3 -m http.server 3000

# In another terminal, expose it
tunnel http 3000 --server localhost:9000 --token devtoken
```

Open `https://dev-<hash>.localhost:443` in your browser (accept self-signed warning).

## Deploy to fly.io (free)

```bash
fly launch --name my-tunnel
fly secrets set TUNNEL_TOKEN=<your-token> TUNNEL_DOMAIN=my-tunnel.fly.dev
fly deploy
```

fly.io handles TLS + domain for free.

## CLI

```
tunnel http <port>     Expose HTTP server
tunnel tcp <port>      Expose TCP server (coming soon)

Flags:
  -s, --subdomain      Custom subdomain
  -s, --server         Tunnel server address (default: localhost:9000)
  -t, --token          Auth token (or TUNNEL_TOKEN env var)
```

## Protocol

Versioned binary framing over TLS:

```
[version: u8] [stream_id: u32 BE] [type: u8] [payload_len: u32 BE] [payload]
```

8 message types: Register, Registered, HttpRequest, HttpResponse, TcpData, Error, CloseStream, Heartbeat.

## Why Rust?

All existing tunnel tools (ngrok, frp, bore) are Go. Rust gives memory safety, zero-cost abstractions, tiny binaries, and a community that loves new tooling.
