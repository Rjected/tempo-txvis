mod ingestor;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use clap::Parser;
use rust_embed::Embed;
use tokio::sync::broadcast;
use tracing::info;

use txviz_api::state::AppState;
use txviz_chain::provider::{ChainProvider, RpcProvider};
use txviz_chain::{EthereumAdapter, detect_chain};
use txviz_chain::tempo::TempoAdapter;
use txviz_chain::adapter::ChainAdapter;
use txviz_core::model::ChainKind;
use txviz_storage::sqlite::SqliteStorage;

use ingestor::IngestorConfig;

#[derive(Embed)]
#[folder = "../../web/dist"]
struct Assets;

/// txviz-server — Transaction dependency graph visualizer
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// HTTP JSON-RPC endpoint (required)
    #[arg(long)]
    rpc_http: String,

    /// WebSocket JSON-RPC endpoint (enables subscriptions)
    #[arg(long)]
    rpc_ws: Option<String>,

    /// RPC request timeout
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    rpc_timeout: std::time::Duration,

    /// Starting block: "latest" or block number
    #[arg(long, default_value = "latest")]
    start_block: String,

    /// Number of historical blocks to process on startup
    #[arg(long, default_value_t = 0)]
    backfill: u64,

    /// Polling interval for new blocks
    #[arg(long, default_value = "2s", value_parser = parse_duration)]
    poll_interval: std::time::Duration,

    /// Force recomputation of existing blocks
    #[arg(long, default_value_t = false)]
    recompute: bool,

    /// Data directory for SQLite + graph files
    #[arg(long, default_value = "./.txviz")]
    data_dir: PathBuf,

    /// Listen address
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: String,

    /// CORS allowed origin
    #[arg(long, default_value = "*")]
    cors_origin: String,

    /// Proxy non-API requests to this URL (for Vite dev server)
    #[arg(long)]
    ui_proxy: Option<String>,

    /// Override chain detection
    #[arg(long, value_parser = parse_chain_kind)]
    force_chain: Option<ChainKind>,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Output logs as JSON
    #[arg(long, default_value_t = false)]
    log_json: bool,
}

fn parse_duration(s: &str) -> Result<std::time::Duration, String> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        ms.parse::<u64>()
            .map(std::time::Duration::from_millis)
            .map_err(|e| format!("invalid duration: {e}"))
    } else if let Some(secs) = s.strip_suffix('s') {
        secs.parse::<u64>()
            .map(std::time::Duration::from_secs)
            .map_err(|e| format!("invalid duration: {e}"))
    } else {
        s.parse::<u64>()
            .map(std::time::Duration::from_secs)
            .map_err(|_| format!("invalid duration '{s}', expected e.g. '30s' or '2000ms'"))
    }
}

fn parse_chain_kind(s: &str) -> Result<ChainKind, String> {
    match s.to_lowercase().as_str() {
        "ethereum" => Ok(ChainKind::Ethereum),
        "tempo" => Ok(ChainKind::Tempo),
        _ => Err(format!("unknown chain kind '{s}', expected 'ethereum' or 'tempo'")),
    }
}

async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try to serve the exact file
    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.to_vec()))
            .unwrap();
    }

    // SPA fallback: serve index.html for non-file paths
    if let Some(content) = Assets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(content.data.to_vec()))
            .unwrap();
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI
    let cli = Cli::parse();

    // 2. Initialize tracing
    let env_filter = tracing_subscriber::EnvFilter::try_new(&cli.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if cli.log_json {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    // 3. Create data_dir
    tokio::fs::create_dir_all(&cli.data_dir)
        .await
        .context("failed to create data directory")?;

    // 4. Initialize SQLite storage
    let storage = SqliteStorage::new(cli.data_dir.clone())
        .await
        .context("failed to initialize storage")?;
    let storage: Arc<dyn txviz_storage::backend::StorageBackend> = Arc::new(storage);

    // 5. Create RPC provider
    let provider = RpcProvider::new(
        cli.rpc_http.clone(),
        cli.rpc_ws.clone(),
        cli.rpc_timeout,
        cli.poll_interval,
        cli.force_chain,
    );
    let provider: Arc<dyn ChainProvider> = Arc::new(provider);

    // 6. Detect chain identity
    let chain_identity = provider
        .chain_identity()
        .await
        .context("failed to detect chain identity")?;

    let chain_kind = detect_chain(
        chain_identity.chain_id,
        &chain_identity.client_version,
        cli.force_chain,
    );

    let adapter: Arc<dyn ChainAdapter> = match chain_kind {
        ChainKind::Tempo => Arc::new(TempoAdapter),
        ChainKind::Ethereum => Arc::new(EthereumAdapter),
    };

    info!(
        chain_id = chain_identity.chain_id,
        chain_kind = ?chain_kind,
        client_version = %chain_identity.client_version,
        "chain identity detected"
    );

    // 7. Create broadcast channel for SSE
    let (live_tx, _) = broadcast::channel(64);

    // 8. Create AppState
    let state = Arc::new(AppState {
        storage: storage.clone(),
        chain_identity,
        live_tx: live_tx.clone(),
    });

    // 9. Start ingestor background task
    let ingestor_provider = provider.clone();
    let ingestor_adapter = adapter.clone();
    let ingestor_storage = storage.clone();
    let ingestor_broadcast = live_tx.clone();
    let ingestor_config = IngestorConfig {
        backfill: cli.backfill,
        recompute: cli.recompute,
    };

    tokio::spawn(async move {
        ingestor::run(
            ingestor_provider,
            ingestor_adapter,
            ingestor_storage,
            ingestor_broadcast,
            ingestor_config,
        )
        .await;
    });

    // 10. Build router
    let app = txviz_api::api_router(state)
        .fallback(static_handler);

    // 11. Print startup banner
    println!("╔══════════════════════════════════════════════╗");
    println!("║  txviz-server v0.1.0                        ║");
    println!("║  Transaction Dependency Visualizer           ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Chain:    {:?}  ║", chain_kind);
    println!("║  RPC:      {}  ", cli.rpc_http);
    println!("║  UI:       http://{}  ", cli.bind);
    println!("║  Data:     {}  ", cli.data_dir.display());
    println!("║  Backfill: {} blocks  ", cli.backfill);
    println!("╚══════════════════════════════════════════════╝");

    // 12. Bind and serve
    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .with_context(|| format!("failed to bind to {}", cli.bind))?;
    info!(bind = %cli.bind, "server listening");

    axum::serve(listener, app)
        .await
        .context("server error")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::try_parse_from([
            "txviz-server",
            "--rpc-http", "http://127.0.0.1:8545",
            "--rpc-ws", "ws://127.0.0.1:8546",
            "--rpc-timeout", "10s",
            "--start-block", "12345",
            "--backfill", "100",
            "--poll-interval", "5s",
            "--recompute",
            "--data-dir", "/tmp/txviz",
            "--bind", "0.0.0.0:3000",
            "--cors-origin", "http://localhost:5173",
            "--force-chain", "tempo",
            "--log-level", "debug",
            "--log-json",
        ])
        .unwrap();

        assert_eq!(cli.rpc_http, "http://127.0.0.1:8545");
        assert_eq!(cli.rpc_ws.as_deref(), Some("ws://127.0.0.1:8546"));
        assert_eq!(cli.rpc_timeout, std::time::Duration::from_secs(10));
        assert_eq!(cli.start_block, "12345");
        assert_eq!(cli.backfill, 100);
        assert_eq!(cli.poll_interval, std::time::Duration::from_secs(5));
        assert!(cli.recompute);
        assert_eq!(cli.data_dir, PathBuf::from("/tmp/txviz"));
        assert_eq!(cli.bind, "0.0.0.0:3000");
        assert_eq!(cli.cors_origin, "http://localhost:5173");
        assert_eq!(cli.force_chain, Some(ChainKind::Tempo));
        assert_eq!(cli.log_level, "debug");
        assert!(cli.log_json);
    }

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::try_parse_from([
            "txviz-server",
            "--rpc-http", "http://127.0.0.1:8545",
        ])
        .unwrap();

        assert_eq!(cli.rpc_http, "http://127.0.0.1:8545");
        assert!(cli.rpc_ws.is_none());
        assert_eq!(cli.rpc_timeout, std::time::Duration::from_secs(30));
        assert_eq!(cli.start_block, "latest");
        assert_eq!(cli.backfill, 0);
        assert_eq!(cli.poll_interval, std::time::Duration::from_secs(2));
        assert!(!cli.recompute);
        assert_eq!(cli.data_dir, PathBuf::from("./.txviz"));
        assert_eq!(cli.bind, "127.0.0.1:8080");
        assert_eq!(cli.cors_origin, "*");
        assert!(cli.ui_proxy.is_none());
        assert!(cli.force_chain.is_none());
        assert_eq!(cli.log_level, "info");
        assert!(!cli.log_json);
    }

    #[test]
    fn test_cli_requires_rpc_http() {
        let result = Cli::try_parse_from(["txviz-server"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30s").unwrap(), std::time::Duration::from_secs(30));
        assert_eq!(parse_duration("2s").unwrap(), std::time::Duration::from_secs(2));
    }

    #[test]
    fn test_parse_duration_millis() {
        assert_eq!(parse_duration("500ms").unwrap(), std::time::Duration::from_millis(500));
    }

    #[test]
    fn test_parse_duration_bare_number() {
        assert_eq!(parse_duration("10").unwrap(), std::time::Duration::from_secs(10));
    }

    #[test]
    fn test_parse_chain_kind_values() {
        assert_eq!(parse_chain_kind("ethereum").unwrap(), ChainKind::Ethereum);
        assert_eq!(parse_chain_kind("tempo").unwrap(), ChainKind::Tempo);
        assert_eq!(parse_chain_kind("Ethereum").unwrap(), ChainKind::Ethereum);
        assert!(parse_chain_kind("unknown").is_err());
    }
}
