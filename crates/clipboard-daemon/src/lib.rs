//! context-clipboardd library surface.
//!
//! The daemon logic lives here so integration tests (and, later, other
//! binaries) can drive the IPC server, DB layer, and handlers directly. The
//! `context-clipboardd` binary (`main.rs`) is a thin wrapper over [`main_entry`].

pub mod capture;
pub mod db;
pub mod frame;
pub mod handlers;
pub mod image_capture;
pub mod lock;
pub mod paths;
pub mod server;
pub mod state;

use crate::paths::Paths;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use clipboard_core::{config::Config, crypto, schema};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "context-clipboardd",
    version,
    about = "Context Clipboard capture daemon"
)]
struct Cli {
    /// Override the data directory (default: platform data dir).
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Run in the foreground (default). Kept for launchd/systemd clarity.
    #[arg(long, global = true)]
    foreground: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the daemon (default when no subcommand is given).
    Run,
}

/// Parse CLI args, initialize logging, and run the requested command.
pub fn main_entry() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run(cli.foreground, cli.data_dir),
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("CONTEXT_CLIPBOARD_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));
    // NOTE: the subscriber never receives clipboard bodies; capture logs only
    // metadata (category, byte size).
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

#[tokio::main]
async fn run(foreground: bool, data_dir_override: Option<PathBuf>) -> Result<()> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        foreground, "starting context-clipboardd"
    );

    let paths = Paths::new(data_dir_override);
    paths
        .ensure_dirs()
        .context("ensuring data/runtime directories")?;
    let data_dir = paths.data_dir()?;

    // Single-instance guard before touching the DB or binding the socket.
    let _instance = lock::SingleInstance::acquire(&paths.lock_path()?)
        .context("acquiring single-instance lock")?;

    // `schema::open` enables WAL + foreign keys and runs migrations.
    let db_path = paths.db_path()?;
    let conn = schema::open(&db_path)
        .map_err(|e| anyhow::anyhow!("opening db {}: {e}", db_path.display()))?;
    info!(db = %db_path.display(), "database ready");

    let config = load_config(&paths)?;
    let key = crypto::load_or_create_dev_key(&paths.key_path()?)
        .map_err(|e| anyhow::anyhow!("loading encryption key: {e}"))?;

    let app_state = Arc::new(state::AppState::new(conn, key, config, data_dir));

    // Install signal handlers *before* the capture backend initializes. The
    // X11 clipboard backend spins up its own thread/connection; registering
    // tokio's signal handlers first keeps graceful shutdown reliable.
    let signal = shutdown_signal()?;

    // Capture thread + shared shutdown flag.
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let capture_handle = capture::spawn(Arc::clone(&app_state), Arc::clone(&shutdown_flag));

    let server = server::Server::bind(&paths.socket_path()?)?;
    let flag = Arc::clone(&shutdown_flag);
    let shutdown = async move {
        signal.await;
        flag.store(true, Ordering::SeqCst);
    };
    server.run(Arc::clone(&app_state), shutdown).await;

    // Stop capture and wait for it to finish its current poll.
    shutdown_flag.store(true, Ordering::SeqCst);
    let _ = capture_handle.join();

    info!("context-clipboardd stopped cleanly");
    Ok(())
}

fn load_config(paths: &Paths) -> Result<Config> {
    let path = paths.config_path()?;
    let config = Config::load(&path).map_err(|e| anyhow::anyhow!("loading config: {e}"))?;
    // Write a default config on first run so users have something to edit.
    if !path.exists() {
        let _ = config.save(&path);
    }
    Ok(config)
}

/// Build a future that resolves when SIGINT or SIGTERM arrives. The underlying
/// signal handlers are registered eagerly (when this is called), so they are in
/// place before the clipboard backend starts its own threads.
#[cfg(unix)]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).context("installing SIGINT handler")?;
    let mut sigterm = signal(SignalKind::terminate()).context("installing SIGTERM handler")?;
    Ok(async move {
        tokio::select! {
            _ = sigint.recv() => info!("received SIGINT"),
            _ = sigterm.recv() => info!("received SIGTERM"),
        }
    })
}

#[cfg(not(unix))]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>> {
    Ok(async {
        let _ = tokio::signal::ctrl_c().await;
    })
}
