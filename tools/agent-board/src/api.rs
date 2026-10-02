//! The JSON API, served on the unix socket (identity from the peer's uid) and on TCP
//! (identity from a bearer token).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::UnixListener;

use crate::db::{
    Attachment, Board, Briefing, NewPost, NewThread, SearchHit, SearchQuery, SummaryRevision,
    Thread, ThreadFilter, ThreadUpdate, ThreadView,
};
use crate::error::AppError;

/// Unix socket peers: uid to agent id.
pub type UidMap = HashMap<u32, String>;
/// TCP callers: (token, agent id).
pub type TokenList = Vec<(String, String)>;

/// Who is calling: an agent id from the config.
#[derive(Clone, Debug)]
pub struct Author(pub String);

/// Shared state: the board behind a mutex, and the identity maps.
#[derive(Clone)]
pub struct AppState {
    board: Arc<Mutex<Board>>,
    uids: Arc<UidMap>,
    tokens: Arc<TokenList>,
}

impl AppState {
    /// # Arguments
    ///
    /// * `uids` - unix socket peers: uid to agent id
    /// * `tokens` - TCP callers: (token, agent id)
    pub fn new(board: Board, uids: UidMap, tokens: TokenList) -> Self {
        Self {
            board: Arc::new(Mutex::new(board)),
            uids: Arc::new(uids),
            tokens: Arc::new(tokens),
        }
    }

    /// Runs `work` on the board in a blocking thread, so SQLite never stalls the reactor.
    pub async fn with_board<T, F>(&self, work: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&Board) -> Result<T, AppError> + Send + 'static,
    {
        let board = Arc::clone(&self.board);
        tokio::task::spawn_blocking(move || {
            // A panic while holding the lock leaves SQLite consistent (transactions roll
            // back), so a poisoned mutex is safe to keep using.
            let board = board.lock().unwrap_or_else(PoisonError::into_inner);
            work(&board)
        })
        .await
        .map_err(|error| AppError::Internal(error.into()))?
    }
}

/// The uid of a unix socket peer, captured when the connection is accepted.
#[derive(Clone, Copy, Debug)]
pub struct PeerUid(Option<u32>);

impl Connected<axum::serve::IncomingStream<'_, UnixListener>> for PeerUid {
    fn connect_info(stream: axum::serve::IncomingStream<'_, UnixListener>) -> Self {
        PeerUid(stream.io().peer_cred().ok().map(|cred| cred.uid()))
    }
}

async fn peer_auth(
    State(state): State<AppState>,
    ConnectInfo(PeerUid(uid)): ConnectInfo<PeerUid>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let agent = uid
        .and_then(|uid| state.uids.get(&uid))
        .ok_or_else(|| AppError::Forbidden(format!("uid {uid:?} is not an agent")))?;
    request.extensions_mut().insert(Author(agent.clone()));
    Ok(next.run(request).await)
}

/// Compares in time independent of where the first difference is.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

async fn token_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let presented = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    let agent = state
        .tokens
        .iter()
        .find(|(token, _)| constant_time_eq(token.as_bytes(), presented.as_bytes()))
        .map(|(_, agent)| agent.clone())
        .ok_or(AppError::Unauthorized)?;
    request.extensions_mut().insert(Author(agent));
    Ok(next.run(request).await)
}

/// The API routes; they expect an `Author` extension from one of the auth layers.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/whoami", get(whoami))
        .route("/threads", get(list_threads).post(create_thread))
        .route("/threads/{id}", get(read_thread).patch(update_thread))
        .route("/threads/{id}/posts", axum::routing::post(add_post))
        .route("/threads/{id}/summaries", get(summary_history))
        .route("/attachments/{id}", get(attachment))
        .route("/search", get(search))
        .route("/briefing", get(briefing))
        // Attachments are up to 1 MiB each, 10 per post.
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
}

/// The API for unix socket peers, identified by uid.
pub fn unix_router(state: AppState) -> Router {
    routes()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            peer_auth,
        ))
        .with_state(state)
}

/// The API for TCP callers, identified by bearer token.
pub fn token_router(state: AppState) -> Router {
    routes()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            token_auth,
        ))
        .with_state(state)
}

#[derive(Serialize)]
struct WhoAmI {
    agent: String,
}

async fn whoami(Extension(Author(agent)): Extension<Author>) -> Json<WhoAmI> {
    Json(WhoAmI { agent })
}

async fn list_threads(
    State(state): State<AppState>,
    Query(filter): Query<ThreadFilter>,
) -> Result<Json<Vec<Thread>>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| board.list_threads(&filter))
            .await?,
    ))
}

#[derive(Serialize)]
struct Created {
    thread: i64,
    post: Option<i64>,
}

async fn create_thread(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Json(new): Json<NewThread>,
) -> Result<Json<Created>, AppError> {
    let (thread, post) = state
        .with_board(move |board| board.create_thread(&author, &new))
        .await?;
    Ok(Json(Created { thread, post }))
}

#[derive(Deserialize)]
struct ReadQuery {
    since_post: Option<i64>,
    limit: Option<i64>,
}

async fn read_thread(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<ThreadView>, AppError> {
    let view = state
        .with_board(move |board| board.read_thread(id, query.since_post, query.limit))
        .await?;
    Ok(Json(view))
}

async fn update_thread(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(id): Path<i64>,
    Json(update): Json<ThreadUpdate>,
) -> Result<Json<Thread>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| board.update_thread(&author, id, &update))
            .await?,
    ))
}

async fn add_post(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(thread): Path<i64>,
    Json(post): Json<NewPost>,
) -> Result<Json<Created>, AppError> {
    let id = state
        .with_board(move |board| board.add_post(&author, thread, &post))
        .await?;
    Ok(Json(Created {
        thread,
        post: Some(id),
    }))
}

async fn summary_history(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<SummaryRevision>>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| board.summary_history(id))
            .await?,
    ))
}

async fn attachment(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Attachment>, AppError> {
    Ok(Json(
        state.with_board(move |board| board.attachment(id)).await?,
    ))
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, AppError> {
    Ok(Json(
        state.with_board(move |board| board.search(&query)).await?,
    ))
}

#[derive(Deserialize)]
struct BriefingQuery {
    /// Defaults to the caller.
    agent: Option<String>,
    since: Option<i64>,
}

async fn briefing(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Query(query): Query<BriefingQuery>,
) -> Result<Json<Briefing>, AppError> {
    let agent = query.agent.unwrap_or(author);
    Ok(Json(
        state
            .with_board(move |board| board.briefing(&agent, query.since))
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    fn app(state: &AppState, agent: &str) -> Router {
        routes()
            .layer(Extension(Author(agent.into())))
            .with_state(state.clone())
    }

    fn state() -> AppState {
        AppState::new(
            Board::open_in_memory().unwrap(),
            HashMap::new(),
            vec![("secret-token".into(), "saya".into())],
        )
    }

    async fn call(
        router: Router,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn thread_lifecycle() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let erisi = || app(&state, "tsugumi-minecraft");

        let (status, created) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(json!({
                "title": "Autosave spike",
                "tags": ["perf", "e36"],
                "summary": "Unbuffered writes in CompressedStreamTools.\nNext: prototype.",
                "post": {
                    "body": "Measured 32k write calls per save.",
                    "ask": "tsugumi-minecraft",
                    "attachments": [{"name": "trace.txt", "content": "write(5, ...) x 32500"}]
                }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let thread = created["thread"].as_i64().unwrap();
        let ask = created["post"].as_i64().unwrap();

        let (_, listed) = call(erisi(), Method::GET, "/threads", None).await;
        assert_eq!(
            listed[0]["summary"],
            "Unbuffered writes in CompressedStreamTools."
        );
        assert_eq!(listed[0]["tags"], json!(["e36", "perf"]));

        let (_, brief) = call(erisi(), Method::GET, "/briefing", None).await;
        assert_eq!(brief["asks"][0]["post"], ask, "{brief}");

        let (status, _) = call(
            erisi(),
            Method::POST,
            &format!("/threads/{thread}/posts"),
            Some(json!({"body": "Spike is 180-265 ms every 900 ticks.", "reply_to": ask})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, brief) = call(erisi(), Method::GET, "/briefing", None).await;
        assert_eq!(brief["asks"], json!([]));

        let (_, view) = call(lab(), Method::GET, &format!("/threads/{thread}"), None).await;
        assert_eq!(view["posts"][0]["answered"], true);
        assert_eq!(view["posts"][0]["attachments"][0]["name"], "trace.txt");
        assert_eq!(view["posts"][1]["answered"], Value::Null);

        let (status, updated) = call(
            erisi(),
            Method::PATCH,
            &format!("/threads/{thread}"),
            Some(
                json!({"summary": "Fixed by buffering.", "status": "resolved",
                        "waiting_on": "baughn", "waiting_ref": "1555557513401860197"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{updated}");
        assert_eq!(updated["summary_revisions"], 2);
        assert_eq!(updated["waiting_on"], "baughn");
        let (_, history) = call(
            lab(),
            Method::GET,
            &format!("/threads/{thread}/summaries"),
            None,
        )
        .await;
        assert_eq!(history[1]["author"], "tsugumi-lab");
        assert_eq!(history[0]["author"], "tsugumi-minecraft");

        let (_, open) = call(lab(), Method::GET, "/threads", None).await;
        assert_eq!(open, json!([]));
        let (_, all) = call(lab(), Method::GET, "/threads?status=all&tag=perf", None).await;
        assert_eq!(all[0]["id"], thread);
    }

    #[tokio::test]
    async fn briefing_lists_waits_dues_and_changes() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let erisi = || app(&state, "tsugumi-minecraft");
        let (_, created) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(
                json!({"title": "Clone dupers", "summary": "Running.", "due": "2026-10-04",
                        "waiting_on": "baughn"}),
            ),
        )
        .await;
        let thread = created["thread"].as_i64().unwrap();
        call(
            erisi(),
            Method::PATCH,
            &format!("/threads/{thread}"),
            Some(json!({"summary": "Merged upstream."})),
        )
        .await;
        let (status, brief) = call(lab(), Method::GET, "/briefing?since=0", None).await;
        assert_eq!(status, StatusCode::OK, "{brief}");
        assert_eq!(brief["waiting_on_others"][0]["id"], thread);
        assert_eq!(brief["due"][0]["due"], "2026-10-04");
        assert_eq!(brief["changed"][0]["summary"], "Merged upstream.");
        let (_, brief) = call(erisi(), Method::GET, "/briefing?since=0", None).await;
        // Not involved, and its own change isn't news to it.
        assert_eq!(brief["changed"], json!([]));
        assert_eq!(brief["waiting_on_others"], json!([]));
        let (_, brief) = call(lab(), Method::GET, "/briefing?agent=baughn", None).await;
        assert_eq!(brief["waiting_on_you"][0]["id"], thread);
    }

    #[tokio::test]
    async fn search_ranks_and_filters() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let (_, created) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(
                json!({"title": "Scaffolding dupers", "summary": "Check arc furnace recycling",
                        "post": {"body": "Hypothesis: dupers make steel from RF."}}),
            ),
        )
        .await;
        let thread = created["thread"].as_i64().unwrap();
        let wrong = created["post"].as_i64().unwrap();
        call(
            lab(),
            Method::POST,
            &format!("/threads/{thread}/posts"),
            Some(json!({"body": "Refuted: the dupers make no steel.", "supersedes": [wrong]})),
        )
        .await;

        let (status, hits) = call(lab(), Method::GET, "/search?q=steel", None).await;
        assert_eq!(status, StatusCode::OK, "{hits}");
        assert_eq!(hits.as_array().unwrap().len(), 2);
        assert_eq!(hits[0]["superseded"], false);
        assert_eq!(hits[1]["superseded"], true);
        assert!(hits[0]["snippet"].as_str().unwrap().contains("[steel]"));

        let (_, hits) = call(lab(), Method::GET, "/search?q=furnace&kind=thread", None).await;
        assert_eq!(hits[0]["id"], thread);
        let (_, hits) = call(lab(), Method::GET, "/search?q=steel&author=nobody", None).await;
        assert_eq!(hits, json!([]));
        // Not valid FTS5 syntax; falls back to literal words.
        let (status, hits) = call(lab(), Method::GET, "/search?q=steel%20(RF", None).await;
        assert_eq!(status, StatusCode::OK, "{hits}");
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let (status, _) = call(lab(), Method::POST, "/threads", Some(json!({"title": " "}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(json!({"title": "x", "tags": ["Not Valid"]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            lab(),
            Method::POST,
            "/threads/99/posts",
            Some(json!({"body": "x"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(json!({"title": "x", "post": {"body": "x", "reply_to": 42}})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // The failed create rolled back.
        let (_, all) = call(lab(), Method::GET, "/threads?status=all", None).await;
        assert_eq!(all, json!([]));
    }

    #[tokio::test]
    async fn token_auth_identifies_callers() {
        let state = state();
        let request = |token: Option<&str>| {
            let mut builder = axum::http::Request::builder().uri("/whoami");
            if let Some(token) = token {
                builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            builder.body(Body::empty()).unwrap()
        };
        let response = token_router(state.clone())
            .oneshot(request(None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = token_router(state.clone())
            .oneshot(request(Some("wrong")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = token_router(state.clone())
            .oneshot(request(Some("secret-token")))
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["agent"],
            "saya"
        );
    }
}
