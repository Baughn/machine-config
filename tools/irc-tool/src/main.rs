use anyhow::{bail, Context, Result};
use axum::{extract::State, routing::post, Json, Router};
use futures::stream::StreamExt;
use irc::client::prelude::*;
use serde::Deserialize;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Deserialize)]
struct Event {
    #[serde(rename = "eventType")]
    event_type: String,
    series: Series,
    episodes: Vec<Episode>,
}

#[derive(Debug, Clone, Deserialize)]
struct Series {
    title: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Episode {
    title: String,
    #[serde(rename = "episodeNumber")]
    episode_number: i32,
    #[serde(rename = "seasonNumber")]
    season_number: i32,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load env file from command line argument or default to .env
    let env_path = std::env::args().nth(1).unwrap_or_else(|| ".env".to_string());
    dotenvy::from_path(&env_path)?;
    
    let channel = dotenvy::var("CHANNEL")?;
    let password = dotenvy::var("PASSWORD")?;

    let (tx, rx) = tokio::sync::mpsc::channel(3);

    let webhook = Router::new()
        .route("/notify-anime", post(notify_anime))
        .with_state(tx);

    let listener = tokio::net::TcpListener::bind("::0:5454").await?;
    let server = axum::serve(listener, webhook);

    let irc = irc_loop(channel, password, rx);

    tokio::select! {
        e = server => {
            e?;
            bail!("Server closed")
        }
        e = irc => {
            e?;
            bail!("IRC connection closed")
        }
    }
}

#[axum::debug_handler]
async fn notify_anime(State(tx): State<mpsc::Sender<Event>>, Json(event): Json<Event>) {
    println!("{:?}", event);
    tx.send(event).await.expect("Failed to send event")
}

async fn irc_loop(
    channel: String,
    password: String,
    mut rx: tokio::sync::mpsc::Receiver<Event>,
) -> Result<()> {
    let config = Config {
        nickname: Some(format!("Moogle")),
        server: Some(format!("irc.rizon.net")),
        port: Some(6697),
        nick_password: Some(password),
        ..Config::default()
    };
    let mut irc = Client::from_config(config)
        .await
        .context("Failed to connect to server")?;
    irc.identify().unwrap();

    let mut stream = irc.stream().unwrap();
    // Start event transmitter.
    let sender = irc.sender();
    let send_to_channel = channel.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if event.event_type != "Download" {
                continue;
            }
            if let Some(ep) = event.episodes.get(0) {
                let message = format!(
                    "Downloaded {} S{}E{}: {}",
                    event.series.title, ep.season_number, ep.episode_number, ep.title,
                );
                sender
                    .send_privmsg(&send_to_channel, &message)
                    .expect("Failed to send message");
            } else {
                let message = "Got an event, but no episodes".to_string();
                sender
                    .send_privmsg(&send_to_channel, &message)
                    .expect("Failed to send message");
            }
        }
    });

    // Start IRC event loop.
    while let Some(message) = stream.next().await.transpose()? {
        println!("{:?}", message);
        match message.command {
            Command::NOTICE(_, ref msg) => {
                let source = message.source_nickname().unwrap_or("unknown");
                if source == "NickServ" && msg.contains("you are now recognized") {
                    irc.send_join(&channel).context("Failed to join channel")?;
                }
            }
            _ => {}
        }
    }

    bail!("IRC connection closed");
}
