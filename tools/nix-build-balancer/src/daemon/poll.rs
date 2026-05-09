use std::sync::Arc;

use crate::config::Config;
use crate::telemetry::poll_remotes;

/// Drive the controller's remote-polling loop.
///
/// `poll_remotes` is synchronous and blocks (it uses `std::thread::sleep` and
/// `std::net::TcpStream`), so we hand it to `spawn_blocking`. The blocking
/// thread owns the whole polling lifetime; tokio's blocking pool is sized for
/// long-lived workers.
pub async fn run(cfg: Arc<Config>) {
    if cfg.remote.is_empty() {
        return;
    }
    let cfg = (*cfg).clone();
    let _ = tokio::task::spawn_blocking(move || poll_remotes(cfg)).await;
}
