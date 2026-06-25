mod tunnel;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "tunnel", about = "Expose localhost to the internet")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Expose an HTTP server
    Http {
        /// Local port to forward
        port: u16,

        /// Custom subdomain (optional)
        #[arg(short, long)]
        subdomain: Option<String>,

        /// Tunnel server address
        #[arg(short, long, default_value = "localhost:9000")]
        server: String,

        /// Auth token
        #[arg(short, long, env = "TUNNEL_TOKEN")]
        token: String,

        /// Config file
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Expose a TCP server
    Tcp {
        /// Local port to forward
        port: u16,

        /// Tunnel server address
        #[arg(short, long, default_value = "localhost:9000")]
        server: String,

        /// Auth token
        #[arg(short, long, env = "TUNNEL_TOKEN")]
        token: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Http {
            port,
            subdomain,
            server,
            token,
            ..
        } => {
            let subdomain = subdomain.unwrap_or_else(|| {
                format!("dev-{}", uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>())
            });
            tunnel::start_http_tunnel(&server, &token, &subdomain, port).await?;
        }
        Commands::Tcp {
            port,
            server,
            token,
        } => {
            tunnel::start_tcp_tunnel(&server, &token, port).await?;
        }
    }

    Ok(())
}
