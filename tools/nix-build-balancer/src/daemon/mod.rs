mod error;
mod listeners;
mod poll;
mod routes;
mod state;

use std::fs;
use std::io;
use std::time::Duration;

use axum::http::StatusCode;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::api::types::telemetry_json;
use crate::config::{Config, Mode};
use crate::persistence::cleanup_state;
use crate::telemetry::read_telemetry;

use state::AppState;

const REQUEST_BODY_LIMIT_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn serve(cfg: Config) -> io::Result<()> {
    install_tracing();

    if cfg.once {
        println!("{}", telemetry_json(&read_telemetry(&cfg.host)?));
        return Ok(());
    }

    fs::create_dir_all(&cfg.data_dir)?;
    cleanup_state(&cfg)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_async(cfg))
}

async fn serve_async(cfg: Config) -> io::Result<()> {
    let state = AppState::new(cfg)?;
    let app = routes::router(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT_BYTES));

    let mut tasks = Vec::new();

    if matches!(state.config.mode, Mode::Controller) && !state.config.remote.is_empty() {
        let cfg = state.config.clone();
        tasks.push(tokio::spawn(async move {
            poll::run(cfg).await;
            Ok(())
        }));
    }

    if let Some(path) = state.config.unix_socket.clone() {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            listeners::serve_unix(path, app).await
        }));
    }

    if let Some(addr) = state.config.listen.clone() {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            listeners::serve_tcp(addr, app).await
        }));
    }

    if tasks.is_empty() {
        return Ok(());
    }

    for handle in tasks {
        if let Err(err) = handle.await {
            tracing::error!(?err, "daemon task panicked");
        }
    }
    Ok(())
}

fn install_tracing() {
    let filter = EnvFilter::try_from_env("NBB_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}
