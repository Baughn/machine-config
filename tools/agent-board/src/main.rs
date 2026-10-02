//! agent-board: a message board for the agent channel's agents.
//!
//! Listeners come from systemd socket activation, by name: `api` (unix socket, caller
//! identified by uid), `http` (TCP, caller identified by bearer token) and `html`
//! (unix socket, read-only pages for the reverse proxy).

mod api;
mod db;
mod discord;
mod error;
mod html;
mod status;

use std::collections::HashMap;
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use serde::Deserialize;

use crate::api::{AppState, PeerUid, TokenList, UidMap};
use crate::db::Board;

#[derive(Parser)]
#[command(about = "Message board for the agent channel")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the API and HTML on the sockets systemd passes in.
    Serve {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        config: PathBuf,
    },
    /// Write a consistent copy of the database to DIR/board-YYYY-MM-DD.db and keep the
    /// newest KEEP copies.
    Backup {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 7)]
        keep: usize,
    },
}

/// The service config, written by the NixOS module.
#[derive(Deserialize)]
struct Config {
    /// Unix user name to agent id, for callers on the `api` socket.
    #[serde(default)]
    users: HashMap<String, String>,
    /// Agent id to the name of a systemd credential holding its token, for `http`.
    #[serde(default)]
    tokens: HashMap<String, String>,
}

/// Looks up a user's uid in /etc/passwd.
fn uid_of(passwd: &str, user: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next() == Some(user))
            .then(|| fields.nth(1)?.parse().ok())
            .flatten()
    })
}

fn load_identities(config: &Config) -> anyhow::Result<(UidMap, TokenList)> {
    let passwd = std::fs::read_to_string("/etc/passwd").context("reading /etc/passwd")?;
    let uids = config
        .users
        .iter()
        .map(|(user, agent)| {
            let uid = uid_of(&passwd, user).with_context(|| format!("no user {user}"))?;
            Ok((uid, agent.clone()))
        })
        .collect::<anyhow::Result<_>>()?;
    let tokens = if config.tokens.is_empty() {
        Vec::new()
    } else {
        let directory = std::env::var_os("CREDENTIALS_DIRECTORY")
            .context("tokens configured but no CREDENTIALS_DIRECTORY")?;
        config
            .tokens
            .iter()
            .map(|(agent, credential)| {
                let path = Path::new(&directory).join(credential);
                let token = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let token = token.trim().to_owned();
                if token.len() < 32 {
                    bail!("token for {agent} is shorter than 32 characters");
                }
                Ok((token, agent.clone()))
            })
            .collect::<anyhow::Result<_>>()?
    };
    Ok((uids, tokens))
}

/// The sockets passed by systemd (sd_listen_fds), keyed by FileDescriptorName.
fn systemd_sockets() -> anyhow::Result<HashMap<String, RawFd>> {
    const FIRST_FD: RawFd = 3;
    let pid: Option<u32> = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|pid| pid.parse().ok());
    if pid != Some(std::process::id()) {
        bail!("no sockets from systemd (run from agent-board.socket units)");
    }
    let count: RawFd = std::env::var("LISTEN_FDS")?.parse()?;
    let names = std::env::var("LISTEN_FDNAMES").unwrap_or_default();
    let names: Vec<&str> = names.split(':').collect();
    if names.len() != count as usize {
        bail!(
            "LISTEN_FDNAMES has {} names for {count} sockets",
            names.len()
        );
    }
    Ok(names
        .into_iter()
        .zip(FIRST_FD..)
        .map(|(name, fd)| (name.to_owned(), fd))
        .collect())
}

async fn serve(db: &Path, config: &Path) -> anyhow::Result<()> {
    let config: Config = serde_json::from_str(
        &std::fs::read_to_string(config)
            .with_context(|| format!("reading {}", config.display()))?,
    )?;
    let (uids, tokens) = load_identities(&config)?;
    let board = Board::open(db).with_context(|| format!("opening {}", db.display()))?;
    let state = AppState::new(board, uids, tokens);

    let mut servers = tokio::task::JoinSet::new();
    for (name, fd) in systemd_sockets()? {
        // SAFETY: systemd passed this fd to us and nothing else in the process owns it.
        match name.as_str() {
            "api" | "html" => {
                let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
                listener.set_nonblocking(true)?;
                let listener = tokio::net::UnixListener::from_std(listener)?;
                if name == "api" {
                    let app = api::unix_router(state.clone())
                        .into_make_service_with_connect_info::<PeerUid>();
                    servers.spawn(async move {
                        axum::serve(listener, app)
                            .with_graceful_shutdown(shutdown())
                            .await
                    });
                } else {
                    let app = html::router(state.clone());
                    servers.spawn(async move {
                        axum::serve(listener, app)
                            .with_graceful_shutdown(shutdown())
                            .await
                    });
                }
            }
            "http" => {
                if config.tokens.is_empty() {
                    bail!("an http socket but no tokens configured");
                }
                let listener = unsafe { std::net::TcpListener::from_raw_fd(fd) };
                listener.set_nonblocking(true)?;
                let listener = tokio::net::TcpListener::from_std(listener)?;
                let app = api::token_router(state.clone());
                servers.spawn(async move {
                    axum::serve(listener, app)
                        .with_graceful_shutdown(shutdown())
                        .await
                });
            }
            other => bail!("unexpected socket {other:?}"),
        }
        tracing::info!("serving {name}");
    }
    while let Some(result) = servers.join_next().await {
        result??;
    }
    Ok(())
}

async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing SIGTERM handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

fn backup(db: &Path, dir: &Path, keep: usize) -> anyhow::Result<()> {
    let board = Board::open(db)?;
    let name = format!("board-{}.db", chrono::Utc::now().format("%Y-%m-%d"));
    let partial = dir.join(format!(".{name}.partial"));
    // VACUUM INTO refuses to overwrite; a leftover from an interrupted run is garbage.
    let _ = std::fs::remove_file(&partial);
    board.backup_to(&partial)?;
    std::fs::rename(&partial, dir.join(&name))?;
    let mut copies: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("board-") && name.ends_with(".db"))
        })
        .collect();
    // The date in the name sorts chronologically.
    copies.sort();
    let excess = copies.len().saturating_sub(keep.max(1));
    for old in &copies[..excess] {
        std::fs::remove_file(old)?;
    }
    tracing::info!("wrote {name}, removed {excess} old copies");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .without_time()
        .init();
    match Cli::parse().command {
        Command::Serve { db, config } => serve(&db, &config).await,
        Command::Backup { db, dir, keep } => {
            tokio::task::spawn_blocking(move || backup(&db, &dir, keep)).await?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_uids() {
        let passwd = "root:x:0:0::/root:/bin/sh\nminecraft:x:1001:100::/home/minecraft:/bin/sh\n";
        assert_eq!(uid_of(passwd, "minecraft"), Some(1001));
        assert_eq!(uid_of(passwd, "root"), Some(0));
        assert_eq!(uid_of(passwd, "mine"), None);
    }

    #[test]
    fn backup_keeps_newest() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("board.db");
        let copies = dir.path().join("backup");
        std::fs::create_dir(&copies).unwrap();
        for day in ["2026-01-01", "2026-01-02", "2026-01-03"] {
            std::fs::write(copies.join(format!("board-{day}.db")), "old").unwrap();
        }
        backup(&db, &copies, 2).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(&copies)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        let today = format!("board-{}.db", chrono::Utc::now().format("%Y-%m-%d"));
        assert_eq!(left, vec!["board-2026-01-03.db".to_owned(), today.clone()]);
        Board::open(&copies.join(today)).unwrap();
    }
}
