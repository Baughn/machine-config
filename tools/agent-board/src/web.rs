//! Browser logins for the HTML view, handed out on Discord (Baughn, msg 1555908517964284076).
//!
//! An admin runs `/board` in Discord; Discord POSTs the interaction to the `interactions`
//! socket, which checks its Ed25519 signature and the member's roles and answers, visible
//! only to that member, with a one-time link. Opening the link and pressing its button
//! trades it for a session cookie. Pages pass with a valid session cookie, or with the
//! header the reverse proxy sets after its own login (Authelia); the proxy strips any
//! copy a client sends.
//!
//! Only SHA-256 hashes of links and sessions are stored.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::header::{COOKIE, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use ed25519_dalek::{Signature, VerifyingKey};
use maud::{html, Markup};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::api::AppState;
use crate::db::{now, Board};
use crate::error::AppError;

/// How long a link from `/board` stays usable.
pub const LOGIN_TTL: i64 = 600;
/// How long a browser stays logged in.
pub const SESSION_TTL: i64 = 30 * 86_400;
/// Set by the reverse proxy once Authelia has let the request through.
pub const PROXY_AUTH_HEADER: &str = "x-board-authelia";
pub const SESSION_COOKIE: &str = "board_session";
/// Discord's signed timestamps must be this fresh, in seconds.
const MAX_SKEW: i64 = 300;
/// last_used is only rewritten when this much older, so page loads rarely write.
const LAST_USED_GRANULARITY: i64 = 300;

/// The `discord` part of the service config.
#[derive(Clone, Debug, Deserialize)]
pub struct DiscordLogin {
    /// The application's public key, hex, from the developer portal.
    pub public_key: String,
    pub guild: String,
    /// Members holding this role get links.
    pub admin_role: String,
    /// Users who get links whatever their roles.
    #[serde(default)]
    pub owners: Vec<String>,
    /// The site's origin, e.g. `https://agents.brage.info`.
    pub base_url: String,
}

#[derive(Debug, Serialize)]
pub struct WebSession {
    pub id: i64,
    pub who: String,
    pub discord_id: String,
    pub created: i64,
    pub expires: i64,
    pub last_used: i64,
}

fn new_token() -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| AppError::Internal(anyhow::anyhow!("getrandom: {error}")))?;
    Ok(hex::encode(bytes))
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Tokens are 64 hex digits; anything else can't match, so skip the lookup.
fn well_formed(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Board {
    /// Stores a fresh one-time login link for a Discord user and returns its token.
    pub fn create_login(&self, who: &str, discord_id: &str) -> Result<String, AppError> {
        let now = now();
        self.conn
            .execute("DELETE FROM web_logins WHERE expires <= ?1", [now])?;
        self.conn
            .execute("DELETE FROM web_sessions WHERE expires <= ?1", [now])?;
        let token = new_token()?;
        self.conn.execute(
            "INSERT INTO web_logins (hash, who, discord_id, created, expires) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![token_hash(&token), who, discord_id, now, now + LOGIN_TTL],
        )?;
        Ok(token)
    }

    /// Whose link this is, if it's still usable. Doesn't use it up.
    pub fn peek_login(&self, token: &str) -> Result<Option<String>, AppError> {
        if !well_formed(token) {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT who FROM web_logins WHERE hash = ?1 AND expires > ?2",
                params![token_hash(token), now()],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Uses up a login link and returns a new session token, or None if the link is
    /// unknown, used or expired.
    pub fn consume_login(&self, token: &str) -> Result<Option<String>, AppError> {
        if !well_formed(token) {
            return Ok(None);
        }
        let now = now();
        let tx = self.conn.unchecked_transaction()?;
        let owner: Option<(String, String)> = tx
            .query_row(
                "DELETE FROM web_logins WHERE hash = ?1 AND expires > ?2 \
                 RETURNING who, discord_id",
                params![token_hash(token), now],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((who, discord_id)) = owner else {
            return Ok(None);
        };
        let session = new_token()?;
        tx.execute(
            "INSERT INTO web_sessions (hash, who, discord_id, created, expires, last_used) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?4)",
            params![
                token_hash(&session),
                who,
                discord_id,
                now,
                now + SESSION_TTL
            ],
        )?;
        tx.commit()?;
        tracing::info!("web login for {who} ({discord_id})");
        Ok(Some(session))
    }

    /// Who a session cookie belongs to, if it's valid.
    pub fn check_session(&self, token: &str) -> Result<Option<String>, AppError> {
        if !well_formed(token) {
            return Ok(None);
        }
        let now = now();
        let found: Option<(i64, String, i64)> = self
            .conn
            .query_row(
                "SELECT id, who, last_used FROM web_sessions WHERE hash = ?1 AND expires > ?2",
                params![token_hash(token), now],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((id, who, last_used)) = found else {
            return Ok(None);
        };
        if now - last_used >= LAST_USED_GRANULARITY {
            self.conn.execute(
                "UPDATE web_sessions SET last_used = ?2 WHERE id = ?1",
                params![id, now],
            )?;
        }
        Ok(Some(who))
    }

    pub fn list_sessions(&self) -> Result<Vec<WebSession>, AppError> {
        let mut statement = self.conn.prepare(
            "SELECT id, who, discord_id, created, expires, last_used FROM web_sessions \
             WHERE expires > ?1 ORDER BY id",
        )?;
        let sessions = statement
            .query_map([now()], |row| {
                Ok(WebSession {
                    id: row.get(0)?,
                    who: row.get(1)?,
                    discord_id: row.get(2)?,
                    created: row.get(3)?,
                    expires: row.get(4)?,
                    last_used: row.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(sessions)
    }

    /// Ends one session, or all of them (and any unused links) when `id` is None.
    /// Returns how many sessions ended.
    pub fn revoke_sessions(&self, id: Option<i64>) -> Result<usize, AppError> {
        Ok(match id {
            Some(id) => self
                .conn
                .execute("DELETE FROM web_sessions WHERE id = ?1", [id])?,
            None => {
                self.conn.execute("DELETE FROM web_logins", [])?;
                self.conn.execute("DELETE FROM web_sessions", [])?
            }
        })
    }
}

/// The value of our session cookie, if the request carries one.
fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == SESSION_COOKIE).then(|| value.to_owned())
        })
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("board_session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0")
}

fn notice(status: StatusCode, title: &str, body: Markup) -> Response {
    (status, crate::html::page(title, "", body)).into_response()
}

/// Middleware for the HTML pages: let through the proxy's Authelia logins and valid
/// sessions; anyone else is told how to get a link.
pub async fn require_login(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.headers().contains_key(PROXY_AUTH_HEADER) {
        return next.run(request).await;
    }
    let cookie = session_cookie(request.headers());
    if let Some(token) = cookie.clone() {
        match state
            .with_board(move |board| board.check_session(&token))
            .await
        {
            Ok(Some(_)) => return next.run(request).await,
            Ok(None) => {}
            Err(error) => return error.into_response(),
        }
    }
    let mut response = notice(
        StatusCode::UNAUTHORIZED,
        "Not logged in",
        html! {
            h3 { "Not logged in" }
            p { "Your session has expired or was revoked. Run " code { "/board" }
                " in the agent channel on Discord for a new login link." }
        },
    );
    if cookie.is_some() {
        // Without the stale cookie, the proxy sends the next visit through Authelia.
        response.headers_mut().insert(SET_COOKIE, clear_cookie());
    }
    response
}

/// `GET /login/{token}`: a button, so link previews and prefetchers can't use the link up.
pub async fn login_page(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, AppError> {
    let lookup = token.clone();
    let who = state
        .with_board(move |board| board.peek_login(&lookup))
        .await?;
    Ok(match who {
        Some(who) => notice(
            StatusCode::OK,
            "Log in",
            html! {
                h3 { "Log in to the agent board" }
                p { "This link is for " b { (who) } ". It works once, and logs this browser in for 30 days." }
                form method="post" action={ "/login/" (token) } {
                    button type="submit" { "Log in" }
                }
            },
        ),
        None => used_link(),
    })
}

fn used_link() -> Response {
    notice(
        StatusCode::NOT_FOUND,
        "Link expired",
        html! {
            h3 { "This link has expired or was already used" }
            p { "Run " code { "/board" } " in the agent channel on Discord for a new one." }
        },
    )
}

/// `POST /login/{token}`: trades the link for a session cookie.
pub async fn login(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, AppError> {
    let session = state
        .with_board(move |board| board.consume_login(&token))
        .await?;
    let Some(session) = session else {
        return Ok(used_link());
    };
    let cookie = format!(
        "{SESSION_COOKIE}={session}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={SESSION_TTL}"
    );
    let cookie = HeaderValue::from_str(&cookie)
        .map_err(|error| AppError::Internal(anyhow::anyhow!("cookie: {error}")))?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(SET_COOKIE, cookie);
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/"));
    Ok(response)
}

/// State for the interactions socket.
pub struct Interactions {
    pub state: AppState,
    pub config: DiscordLogin,
    pub key: VerifyingKey,
}

impl Interactions {
    pub fn new(state: AppState, config: DiscordLogin) -> anyhow::Result<Self> {
        let bytes: [u8; 32] = hex::decode(config.public_key.trim())?
            .try_into()
            .map_err(|_| anyhow::anyhow!("discord.public_key is not 32 bytes"))?;
        let key = VerifyingKey::from_bytes(&bytes)?;
        Ok(Self { state, config, key })
    }
}

/// The interactions route, for Discord via the reverse proxy.
pub fn interactions_router(interactions: Interactions) -> Router {
    Router::new()
        .route("/discord/interactions", post(interaction))
        .with_state(Arc::new(interactions))
}

/// Checks Discord's signature over timestamp + body, and that the timestamp is fresh.
fn verify(key: &VerifyingKey, headers: &HeaderMap, body: &[u8], now: i64) -> bool {
    let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
    let (Some(signature), Some(timestamp)) = (
        header("x-signature-ed25519"),
        header("x-signature-timestamp"),
    ) else {
        return false;
    };
    let Ok(signature) = hex::decode(signature) else {
        return false;
    };
    let Ok(signature) = <[u8; 64]>::try_from(signature) else {
        return false;
    };
    if !timestamp
        .parse::<i64>()
        .is_ok_and(|sent| (now - sent).abs() <= MAX_SKEW)
    {
        return false;
    }
    let mut message = timestamp.as_bytes().to_vec();
    message.extend_from_slice(body);
    key.verify_strict(&message, &Signature::from_bytes(&signature))
        .is_ok()
}

/// The (user id, name) of an interaction's member, if they may have a link.
fn admin(config: &DiscordLogin, interaction: &Value) -> Option<(String, String)> {
    if interaction["guild_id"].as_str() != Some(config.guild.as_str()) {
        return None;
    }
    let member = &interaction["member"];
    let id = member["user"]["id"].as_str()?;
    let is_admin = member["roles"].as_array().is_some_and(|roles| {
        roles
            .iter()
            .any(|role| role.as_str() == Some(&config.admin_role))
    });
    if !is_admin && !config.owners.iter().any(|owner| owner == id) {
        return None;
    }
    let name = member["user"]["username"].as_str().unwrap_or(id);
    Some((id.to_owned(), name.to_owned()))
}

/// Ephemeral (64) and without link previews (4).
fn private_reply(content: &str) -> Response {
    Json(json!({ "type": 4, "data": { "content": content, "flags": 64 | 4 } })).into_response()
}

async fn interaction(
    State(interactions): State<Arc<Interactions>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !verify(&interactions.key, &headers, &body, now()) {
        return (StatusCode::UNAUTHORIZED, "invalid request signature").into_response();
    }
    let Ok(interaction) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match interaction["type"].as_i64() {
        // PING, sent when the endpoint URL is saved in the developer portal.
        Some(1) => Json(json!({ "type": 1 })).into_response(),
        Some(2) if interaction["data"]["name"] == "board" => {
            let Some((id, name)) = admin(&interactions.config, &interaction) else {
                return private_reply("Board links are for admins only.");
            };
            let who = name.clone();
            let token = match interactions
                .state
                .with_board(move |board| board.create_login(&who, &id))
                .await
            {
                Ok(token) => token,
                Err(error) => return error.into_response(),
            };
            tracing::info!("board link issued to {name}");
            private_reply(&format!(
                "Your agent board login link: <{}/login/{token}>\n\
                 It works once within 10 minutes, and logs that browser in for 30 days. \
                 Don't share it.",
                interactions.config.base_url.trim_end_matches('/')
            ))
        }
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn config() -> DiscordLogin {
        DiscordLogin {
            public_key: String::new(),
            guild: "1".into(),
            admin_role: "10".into(),
            owners: vec!["99".into()],
            base_url: "https://board.test".into(),
        }
    }

    #[test]
    fn signatures() {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let key = signing.verifying_key();
        let body = br#"{"type":1}"#;
        let signed = |timestamp: &str, body: &[u8]| {
            let mut message = timestamp.as_bytes().to_vec();
            message.extend_from_slice(body);
            let mut headers = HeaderMap::new();
            headers.insert(
                "x-signature-ed25519",
                hex::encode(signing.sign(&message).to_bytes())
                    .parse()
                    .unwrap(),
            );
            headers.insert("x-signature-timestamp", timestamp.parse().unwrap());
            headers
        };
        assert!(verify(&key, &signed("1000", body), body, 1100));
        // Tampered body, stale timestamp, missing headers.
        assert!(!verify(&key, &signed("1000", body), br#"{"type":2}"#, 1100));
        assert!(!verify(&key, &signed("1000", body), body, 2000));
        assert!(!verify(&key, &HeaderMap::new(), body, 1000));
        let other = SigningKey::from_bytes(&[8; 32]).verifying_key();
        assert!(!verify(&other, &signed("1000", body), body, 1100));
    }

    #[test]
    fn only_admins_and_owners() {
        let config = config();
        let member = |guild: &str, id: &str, roles: &[&str]| json!({ "guild_id": guild, "member": { "user": { "id": id, "username": "u" }, "roles": roles } });
        assert_eq!(
            admin(&config, &member("1", "5", &["3", "10"])),
            Some(("5".into(), "u".into()))
        );
        assert!(admin(&config, &member("1", "99", &[])).is_some());
        assert!(admin(&config, &member("1", "5", &["3"])).is_none());
        assert!(admin(&config, &member("2", "5", &["10"])).is_none());
        assert!(admin(&config, &json!({ "guild_id": "1", "user": { "id": "99" } })).is_none());
    }

    #[test]
    fn links_work_once() {
        let board = Board::open_in_memory().unwrap();
        let link = board.create_login("vindex", "5").unwrap();
        assert_eq!(board.peek_login(&link).unwrap().as_deref(), Some("vindex"));
        let session = board.consume_login(&link).unwrap().unwrap();
        assert!(board.consume_login(&link).unwrap().is_none());
        assert!(board.peek_login(&link).unwrap().is_none());
        assert_eq!(
            board.check_session(&session).unwrap().as_deref(),
            Some("vindex")
        );
        assert!(board.check_session(&link).unwrap().is_none());
        assert!(board.check_session("nonsense").unwrap().is_none());
        let listed = board.list_sessions().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(board.revoke_sessions(Some(listed[0].id)).unwrap(), 1);
        assert!(board.check_session(&session).unwrap().is_none());
    }

    #[test]
    fn expired_links_and_sessions_fail() {
        let board = Board::open_in_memory().unwrap();
        let link = board.create_login("a", "5").unwrap();
        board
            .conn
            .execute("UPDATE web_logins SET expires = 0", [])
            .unwrap();
        assert!(board.consume_login(&link).unwrap().is_none());
        let link = board.create_login("a", "5").unwrap();
        let session = board.consume_login(&link).unwrap().unwrap();
        board
            .conn
            .execute("UPDATE web_sessions SET expires = 0", [])
            .unwrap();
        assert!(board.check_session(&session).unwrap().is_none());
    }

    #[tokio::test]
    async fn pages_need_a_login() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let board = Board::open_in_memory().unwrap();
        let link = board.create_login("vindex", "5").unwrap();
        let app = crate::html::router(AppState::new(board, Default::default(), Vec::new()));
        let send = |method: &str, uri: &str, header: Option<(&'static str, String)>| {
            let mut request = Request::builder().method(method).uri(uri);
            if let Some((name, value)) = header {
                request = request.header(name, value);
            }
            app.clone().oneshot(request.body(Body::empty()).unwrap())
        };

        let response = send("GET", "/status", None).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(SET_COOKIE).is_none());
        let response = send("GET", "/status", Some((PROXY_AUTH_HEADER, "1".into())))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // A stale cookie is cleared, so the proxy falls back to Authelia.
        let stale = Some(("cookie", format!("{SESSION_COOKIE}={link}")));
        let response = send("GET", "/status", stale).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers()[SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0"));

        // Looking at the link doesn't use it up; pressing the button does, once.
        let page = format!("/login/{link}");
        assert_eq!(
            send("GET", &page, None).await.unwrap().status(),
            StatusCode::OK
        );
        let response = send("POST", &page, None).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response.headers()[SET_COOKIE].to_str().unwrap().to_owned();
        assert!(
            cookie.contains("HttpOnly") && cookie.contains("Secure"),
            "{cookie}"
        );
        let session = cookie.split(';').next().unwrap().to_owned();
        assert_eq!(
            send("POST", &page, None).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        let response = send("GET", "/status", Some(("cookie", session)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn finds_the_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, "a=1; board_session=abc; b=2".parse().unwrap());
        assert_eq!(session_cookie(&headers).as_deref(), Some("abc"));
        headers.insert(COOKIE, "board_sessionx=1".parse().unwrap());
        assert_eq!(session_cookie(&headers), None);
    }
}
