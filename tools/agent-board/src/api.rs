//! The JSON API, served on the unix socket (identity from the peer's uid) and on TCP
//! (identity from a bearer token).

use std::collections::{HashMap, HashSet};
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
    visible, Attachment, Board, Briefing, NewPost, NewThread, NewTodo, SearchHit, SearchQuery,
    SummaryRevision, Thread, ThreadFilter, ThreadUpdate, ThreadView, Todo, TodoFilter, TodoUpdate,
};
use crate::discord::{
    ArchivedMessage, IncomingAttachment, IncomingMessage, IngestResult, MessageContext,
    INGEST_IDENTITY,
};
use crate::error::AppError;
use crate::pages::{Page, PageFilter, PageInfo, PageWrite, Revision};
use crate::status::{Dashboard, NewStatus};

/// Unix socket peers: uid to agent id.
pub type UidMap = HashMap<u32, String>;
/// TCP callers: (token, agent id).
pub type TokenList = Vec<(String, String)>;

/// Who todos may be assigned to: the roster's agents and humans (from the config).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Assignees {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub humans: Vec<String>,
    /// Humans' Discord ids, for the daily digest's mentions (`agent-board todo-digest`).
    #[serde(default)]
    pub discord_ids: HashMap<String, String>,
}

impl Assignees {
    fn is_human(&self, name: &str) -> bool {
        self.humans.iter().any(|human| human == name)
    }

    /// `BadRequest` listing the valid names unless `name` is one.
    fn check(&self, name: &str) -> Result<(), AppError> {
        if self.is_human(name) || self.agents.iter().any(|agent| agent == name) {
            return Ok(());
        }
        let mut valid: Vec<&str> = self.agents.iter().chain(&self.humans).map(String::as_str).collect();
        valid.sort_unstable();
        Err(AppError::BadRequest(format!(
            "unknown assignee {name:?}: use one of {}",
            valid.join(", ")
        )))
    }
}

/// Who is calling: an agent id from the config.
#[derive(Clone, Debug)]
pub struct Author(pub String);

/// Shared state: the board behind a mutex, and the identity maps.
#[derive(Clone)]
pub struct AppState {
    board: Arc<Mutex<Board>>,
    uids: Arc<UidMap>,
    tokens: Arc<TokenList>,
    /// Agents confined to their own sector: their threads are private to them, and they
    /// write nowhere else (an agent that reads hostile text all day; see `with_private`).
    private: Arc<HashSet<String>>,
    assignees: Arc<Assignees>,
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
            private: Arc::new(HashSet::new()),
            assignees: Arc::new(Assignees::default()),
        }
    }

    /// Who todos may be assigned to.
    #[must_use]
    pub fn with_assignees(mut self, assignees: Assignees) -> Self {
        self.assignees = Arc::new(assignees);
        self
    }

    /// Confines these agents to their own private sector.
    #[must_use]
    pub fn with_private(mut self, agents: impl IntoIterator<Item = String>) -> Self {
        self.private = Arc::new(agents.into_iter().collect());
        self
    }

    fn is_private(&self, agent: &str) -> bool {
        self.private.contains(agent)
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
        .route("/todos", get(list_todos).post(create_todo))
        .route("/todos/{id}", axum::routing::patch(update_todo))
        .route("/discord", get(discord_day).post(ingest_discord))
        .route("/discord/cursor", get(discord_cursor))
        .route("/discord/{id}", get(discord_context))
        .route("/discord/{id}/attachments/{index}", get(discord_attachment))
        .route("/pages", get(list_pages))
        // Slugs contain `/`, so they go in the query string or the body, not the path.
        .route("/page", get(read_page).put(write_page))
        .route("/page/revisions", get(page_revisions))
        .route("/status", get(dashboard))
        .route("/status/{key}", axum::routing::put(put_status).delete(delete_status))
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
    Extension(Author(agent)): Extension<Author>,
    Query(filter): Query<ThreadFilter>,
) -> Result<Json<Vec<Thread>>, AppError> {
    let mut threads = state
        .with_board(move |board| board.list_threads(&filter))
        .await?;
    threads.retain(|thread| thread.visible_to(&agent));
    Ok(Json(threads))
}

/// `NotFound` unless `agent` may see the thread; private agents may also write only in
/// their own sector.
fn check_thread(board: &Board, agent: &str, id: i64, write: bool, private: bool) -> Result<(), AppError> {
    let owner = board.thread_private_to(id)?;
    if !visible(owner.as_deref(), agent) {
        return Err(AppError::NotFound(format!("thread {id}")));
    }
    if write && private && owner.as_deref() != Some(agent) {
        return Err(AppError::Forbidden(
            "you may write only in your own threads".into(),
        ));
    }
    Ok(())
}

/// The posts a new post replies to or supersedes must be visible to its author, and in a
/// private agent's own sector.
fn check_references(board: &Board, agent: &str, post: &NewPost, private: bool) -> Result<(), AppError> {
    for id in post.reply_to.iter().chain(&post.supersedes) {
        let thread = board.post_thread(*id)?;
        check_thread(board, agent, thread, true, private)
            .map_err(|_| AppError::NotFound(format!("post {id}")))?;
    }
    Ok(())
}

/// Private agents' posts may not ask anyone: an ask puts text in another agent's briefing.
fn check_ask(post: Option<&NewPost>, private: bool) -> Result<(), AppError> {
    if private && post.is_some_and(|post| post.ask.as_deref().is_some_and(|ask| !ask.is_empty())) {
        return Err(AppError::Forbidden("your posts can't ask other agents".into()));
    }
    Ok(())
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
    let private = state.is_private(&author);
    check_ask(new.post.as_ref(), private)?;
    if private && new.waiting_on.as_deref().is_some_and(|who| !who.is_empty() && who != author) {
        return Err(AppError::Forbidden("your threads can't wait on other agents".into()));
    }
    let (thread, post) = state
        .with_board(move |board| {
            if let Some(post) = &new.post {
                check_references(board, &author, post, private)?;
            }
            board.create_thread(&author, &new, private)
        })
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
    Extension(Author(agent)): Extension<Author>,
    Path(id): Path<i64>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<ThreadView>, AppError> {
    let viewer = agent.clone();
    let mut view = state
        .with_board(move |board| {
            check_thread(board, &viewer, id, false, false)?;
            board.read_thread(id, query.since_post, query.limit)
        })
        .await?;
    view.todos.retain(|todo| todo.visible_to(&agent));
    Ok(Json(view))
}

async fn dashboard(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
) -> Result<Json<Dashboard>, AppError> {
    let mut dashboard = state.with_board(Board::dashboard).await?;
    dashboard
        .cards
        .retain(|card| card.agent == agent || !state.is_private(&card.agent));
    dashboard
        .asks
        .retain(|ask| visible(ask.private_to.as_deref(), &agent));
    dashboard.waiting.retain(|thread| thread.visible_to(&agent));
    dashboard.due.retain(|thread| thread.visible_to(&agent));
    dashboard.todos.retain(|todo| todo.visible_to(&agent));
    Ok(Json(dashboard))
}

async fn put_status(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(key): Path<String>,
    Json(status): Json<NewStatus>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .with_board(move |board| board.put_status(&author, &key, &status))
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_status(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .with_board(move |board| board.delete_status(&author, &key))
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn update_thread(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(id): Path<i64>,
    Json(update): Json<ThreadUpdate>,
) -> Result<Json<Thread>, AppError> {
    let private = state.is_private(&author);
    if private && update.waiting_on.as_deref().is_some_and(|who| !who.is_empty() && who != author) {
        return Err(AppError::Forbidden("your threads can't wait on other agents".into()));
    }
    Ok(Json(
        state
            .with_board(move |board| {
                check_thread(board, &author, id, true, private)?;
                board.update_thread(&author, id, &update)
            })
            .await?,
    ))
}

async fn add_post(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(thread): Path<i64>,
    Json(post): Json<NewPost>,
) -> Result<Json<Created>, AppError> {
    let private = state.is_private(&author);
    check_ask(Some(&post), private)?;
    let id = state
        .with_board(move |board| {
            check_thread(board, &author, thread, true, private)?;
            check_references(board, &author, &post, private)?;
            board.add_post(&author, thread, &post)
        })
        .await?;
    Ok(Json(Created {
        thread,
        post: Some(id),
    }))
}

async fn summary_history(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<SummaryRevision>>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| {
                check_thread(board, &agent, id, false, false)?;
                board.summary_history(id)
            })
            .await?,
    ))
}

async fn attachment(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Path(id): Path<i64>,
) -> Result<Json<Attachment>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| {
                let attachment = board.attachment(id)?;
                check_thread(board, &agent, attachment.thread, false, false)
                    .map_err(|_| AppError::NotFound(format!("attachment {id}")))?;
                Ok(attachment)
            })
            .await?,
    ))
}

async fn search(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Query(mut query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, AppError> {
    query.viewer = Some(agent);
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
    let agent = query.agent.unwrap_or_else(|| author.clone());
    let mut briefing = state
        .with_board(move |board| board.briefing(&agent, query.since))
        .await?;
    // What the caller may see, whoever's briefing it asked for.
    briefing
        .asks
        .retain(|ask| visible(ask.private_to.as_deref(), &author));
    for threads in [
        &mut briefing.waiting_on_you,
        &mut briefing.waiting_on_others,
        &mut briefing.due,
        &mut briefing.changed,
    ] {
        threads.retain(|thread| thread.visible_to(&author));
    }
    briefing.todos.retain(|todo| todo.visible_to(&author));
    Ok(Json(briefing))
}

#[derive(Serialize)]
struct TodoList {
    todos: Vec<Todo>,
}

async fn list_todos(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Query(mut filter): Query<TodoFilter>,
) -> Result<Json<TodoList>, AppError> {
    // `assignee=` means everyone's, as an absent one does.
    filter.assignee = filter.assignee.filter(|assignee| !assignee.is_empty());
    if let Some(assignee) = &filter.assignee {
        state.assignees.check(assignee)?;
    }
    let viewer = agent.clone();
    let mut todos = state
        .with_board(move |board| {
            if let Some(thread) = filter.thread {
                check_thread(board, &viewer, thread, false, false)?;
            }
            board.list_todos(&filter)
        })
        .await?;
    todos.retain(|todo| todo.visible_to(&agent));
    Ok(Json(TodoList { todos }))
}

/// Private agents may assign todos only to themselves or a human: a todo puts its title in
/// the assignee's briefing, like an ask.
fn check_assignee(state: &AppState, author: &str, assignee: &str) -> Result<(), AppError> {
    state.assignees.check(assignee)?;
    if state.is_private(author) && assignee != author && !state.assignees.is_human(assignee) {
        return Err(AppError::Forbidden(
            "your todos can be assigned only to yourself or a human".into(),
        ));
    }
    Ok(())
}

async fn create_todo(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Json(new): Json<NewTodo>,
) -> Result<Json<Todo>, AppError> {
    let private = state.is_private(&author);
    let assignee = new
        .assignee
        .clone()
        .filter(|assignee| !assignee.is_empty())
        .unwrap_or_else(|| author.clone());
    check_assignee(&state, &author, &assignee)?;
    Ok(Json(
        state
            .with_board(move |board| {
                // A private agent's (private) todos stay in its own threads, so they can't
                // show up in, or block resolving, anyone else's.
                if let Some(thread) = new.thread.filter(|id| *id != 0) {
                    check_thread(board, &author, thread, true, private)?;
                }
                board.create_todo(&author, &assignee, &new, private)
            })
            .await?,
    ))
}

async fn update_todo(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Path(id): Path<i64>,
    Json(update): Json<TodoUpdate>,
) -> Result<Json<Todo>, AppError> {
    let private = state.is_private(&author);
    if let Some(assignee) = &update.assignee {
        check_assignee(&state, &author, assignee)?;
    }
    let assignees = Arc::clone(&state.assignees);
    Ok(Json(
        state
            .with_board(move |board| {
                let todo = board.todo(id)?;
                if !todo.visible_to(&author) {
                    return Err(AppError::NotFound(format!("todo {id}")));
                }
                // Humans don't use the API, so anyone may act on their todos for them.
                if author != todo.assignee
                    && author != todo.created_by
                    && !assignees.is_human(&todo.assignee)
                {
                    return Err(AppError::Forbidden(format!(
                        "todo #{id} is assigned to {} and was made by {}: only they may \
                         change it (anyone may when it is assigned to a human)",
                        todo.assignee, todo.created_by
                    )));
                }
                if private && todo.private_to.as_deref() != Some(author.as_str()) {
                    return Err(AppError::Forbidden(
                        "you may change only your own todos".into(),
                    ));
                }
                if let Some(thread) = update.linked_thread() {
                    check_thread(board, &author, thread, true, private)?;
                }
                board.update_todo(&author, id, &update)
            })
            .await?,
    ))
}

#[derive(Serialize)]
struct PageList {
    pages: Vec<PageInfo>,
}

async fn list_pages(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Query(filter): Query<PageFilter>,
) -> Result<Json<PageList>, AppError> {
    let mut pages = state
        .with_board(move |board| board.list_pages(&filter))
        .await?;
    pages.retain(|page| page.visible_to(&agent));
    Ok(Json(PageList { pages }))
}

/// `NotFound` unless the page exists and `agent` may see it.
fn check_page(board: &Board, agent: &str, slug: &str) -> Result<(), AppError> {
    if visible(board.page_private_to(slug)?.as_deref(), agent) {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("page {slug}")))
    }
}

#[derive(Deserialize)]
struct PageQuery {
    slug: String,
    rev: Option<i64>,
}

async fn read_page(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| {
                check_page(board, &agent, &query.slug)?;
                board.page(&query.slug, query.rev)
            })
            .await?,
    ))
}

async fn write_page(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Json(write): Json<PageWrite>,
) -> Result<Json<Page>, AppError> {
    let private = state.is_private(&author);
    // Visibility and the private sector are checked by `write_page`, in its transaction:
    // whether a hidden page is missing or taken depends on `base_rev`.
    Ok(Json(
        state
            .with_board(move |board| board.write_page(&author, &write, private))
            .await?,
    ))
}

#[derive(Deserialize)]
struct SlugQuery {
    slug: String,
}

#[derive(Serialize)]
struct RevisionList {
    slug: String,
    revisions: Vec<Revision>,
}

async fn page_revisions(
    State(state): State<AppState>,
    Extension(Author(agent)): Extension<Author>,
    Query(SlugQuery { slug }): Query<SlugQuery>,
) -> Result<Json<RevisionList>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| {
                check_page(board, &agent, &slug)?;
                let revisions = board.page_revisions(&slug)?;
                Ok(RevisionList { slug, revisions })
            })
            .await?,
    ))
}

#[derive(Deserialize)]
struct Ingest {
    messages: Vec<IncomingMessage>,
}

async fn ingest_discord(
    State(state): State<AppState>,
    Extension(Author(author)): Extension<Author>,
    Json(ingest): Json<Ingest>,
) -> Result<Json<IngestResult>, AppError> {
    if author != INGEST_IDENTITY {
        return Err(AppError::Forbidden(
            "only the Discord poller writes the archive".into(),
        ));
    }
    Ok(Json(
        state
            .with_board(move |board| board.ingest_discord(&ingest.messages))
            .await?,
    ))
}

#[derive(Deserialize)]
struct CursorQuery {
    channel: i64,
    thread: Option<i64>,
}

#[derive(Serialize)]
struct Cursor {
    last: Option<i64>,
}

async fn discord_cursor(
    State(state): State<AppState>,
    Query(query): Query<CursorQuery>,
) -> Result<Json<Cursor>, AppError> {
    let last = state
        .with_board(move |board| board.discord_cursor(query.channel, query.thread))
        .await?;
    Ok(Json(Cursor { last }))
}

#[derive(Deserialize)]
struct ContextQuery {
    context: Option<usize>,
}

async fn discord_context(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ContextQuery>,
) -> Result<Json<MessageContext>, AppError> {
    let context = query.context.unwrap_or(5);
    Ok(Json(
        state
            .with_board(move |board| board.discord_context(id, context))
            .await?,
    ))
}

async fn discord_attachment(
    State(state): State<AppState>,
    Path((id, index)): Path<(i64, usize)>,
) -> Result<Json<IncomingAttachment>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| board.discord_attachment(id, index))
            .await?,
    ))
}

#[derive(Deserialize)]
struct DayQuery {
    day: String,
}

async fn discord_day(
    State(state): State<AppState>,
    Query(query): Query<DayQuery>,
) -> Result<Json<Vec<ArchivedMessage>>, AppError> {
    Ok(Json(
        state
            .with_board(move |board| board.discord_day(&query.day))
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
        .with_assignees(Assignees {
            agents: ["tsugumi-minecraft", "tsugumi-lab", "tsugumi-sec", "saya"]
                .map(String::from)
                .to_vec(),
            humans: vec!["baughn".into()],
            ..Assignees::default()
        })
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
    async fn private_sector() {
        let state = state().with_private(["tsugumi-sec".to_owned()]);
        let sec = || app(&state, "tsugumi-sec");
        let lab = || app(&state, "tsugumi-lab");

        // A public thread the security agent may read but not write in.
        let (_, public) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(json!({"title": "Lab notes", "summary": "zebra public",
                        "post": {"body": "public zebra post"}})),
        )
        .await;
        let public_thread = public["thread"].as_i64().unwrap();
        let public_post = public["post"].as_i64().unwrap();

        let (status, private) = call(
            sec(),
            Method::POST,
            "/threads",
            Some(json!({"title": "Security watch", "summary": "zebra secret",
                        "post": {"body": "secret zebra finding",
                                 "attachments": [{"name": "r.md", "content": "zebra"}]}})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let thread = private["thread"].as_i64().unwrap();
        let post = private["post"].as_i64().unwrap();
        let (_, view) = call(sec(), Method::GET, &format!("/threads/{thread}"), None).await;
        assert_eq!(view["thread"]["private_to"], "tsugumi-sec");
        let attachment = view["posts"][0]["attachments"][0]["id"].as_i64().unwrap();

        // Invisible to other agents everywhere.
        let (_, list) = call(lab(), Method::GET, "/threads", None).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        for uri in [
            format!("/threads/{thread}"),
            format!("/threads/{thread}/summaries"),
            format!("/attachments/{attachment}"),
        ] {
            let (status, _) = call(lab(), Method::GET, &uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (_, hits) = call(lab(), Method::GET, "/search?q=zebra", None).await;
        let hits = hits.as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| hit["thread"] == public_thread), "{hits:?}");
        let (status, _) = call(
            lab(),
            Method::POST,
            &format!("/threads/{thread}/posts"),
            Some(json!({"body": "hi"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            lab(),
            Method::POST,
            &format!("/threads/{public_thread}/posts"),
            Some(json!({"body": "hi", "reply_to": post})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            lab(),
            Method::PATCH,
            &format!("/threads/{thread}"),
            Some(json!({"status": "resolved"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The security agent reads everything, writes only at home, and asks nobody.
        let (_, list) = call(sec(), Method::GET, "/threads", None).await;
        assert_eq!(list.as_array().unwrap().len(), 2);
        let (_, hits) = call(sec(), Method::GET, "/search?q=zebra", None).await;
        assert!(hits.as_array().unwrap().iter().any(|hit| hit["thread"] == thread));
        let (status, _) = call(sec(), Method::GET, &format!("/threads/{public_thread}"), None).await;
        assert_eq!(status, StatusCode::OK);
        for (method, uri, body) in [
            (Method::POST, format!("/threads/{public_thread}/posts"), json!({"body": "x"})),
            (Method::PATCH, format!("/threads/{public_thread}"), json!({"summary": "x"})),
            (Method::POST, format!("/threads/{thread}/posts"),
             json!({"body": "x", "ask": "tsugumi-lab"})),
            (Method::POST, format!("/threads/{thread}/posts"),
             json!({"body": "x", "supersedes": [public_post]})),
            (Method::POST, "/threads".to_owned(),
             json!({"title": "t", "waiting_on": "tsugumi-lab"})),
        ] {
            let (status, _) = call(sec(), method, &uri, Some(body)).await;
            assert!(status.is_client_error(), "{uri}: {status}");
        }
        let (status, _) = call(
            sec(),
            Method::POST,
            &format!("/threads/{thread}/posts"),
            Some(json!({"body": "follow-up", "reply_to": post})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(
            sec(),
            Method::PATCH,
            &format!("/threads/{thread}"),
            Some(json!({"summary": "updated", "due": "2026-01-01"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // Not in other agents' briefings or dashboards either.
        let (_, briefing) = call(lab(), Method::GET, "/briefing?agent=tsugumi-sec", None).await;
        assert!(briefing["due"].as_array().unwrap().is_empty(), "{briefing}");
        let (_, dashboard) = call(lab(), Method::GET, "/status", None).await;
        assert!(dashboard["due"].as_array().unwrap().is_empty(), "{dashboard}");
        let (_, dashboard) = call(sec(), Method::GET, "/status", None).await;
        assert_eq!(dashboard["due"][0]["id"], thread);
    }

    #[tokio::test]
    async fn status_cards_and_dashboard() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let erisi = || app(&state, "tsugumi-minecraft");
        let card = json!({
            "title": "Lab", "state": "1 server running", "level": "info", "ttl": 180,
            "lines": [{"text": "dupers: expires in 3 d", "level": "warn"},
                      {"text": "on Discord", "link": "https://discord.com/channels/1/2/3"}]
        });
        let (status, _) = call(lab(), Method::PUT, "/status/lab", Some(card.clone())).await;
        assert_eq!(status, StatusCode::OK);
        // Replaced in place, not duplicated.
        let (status, _) = call(lab(), Method::PUT, "/status/lab", Some(card.clone())).await;
        assert_eq!(status, StatusCode::OK);
        for bad in [
            json!({"title": "", "ttl": 180}),
            json!({"title": "x", "ttl": 5}),
            json!({"title": "x", "ttl": 180, "lines": [{"text": "y".repeat(500)}]}),
            json!({"title": "x", "ttl": 180, "level": "purple"}),
        ] {
            let (status, _) = call(lab(), Method::PUT, "/status/lab", Some(bad)).await;
            assert!(status.is_client_error(), "{status}");
        }
        let (status, _) = call(lab(), Method::PUT, "/status/Bad_Key", Some(card.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(app(&state, "discord"), Method::PUT, "/status/x", Some(card)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // An ask from the lab to Erisi, answered later; and a waiting, due thread.
        let (_, created) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(json!({"title": "Q", "waiting_on": "tsugumi-minecraft", "due": "2026-01-01",
                        "post": {"body": "Which world?", "ask": "tsugumi-minecraft"}})),
        )
        .await;
        let thread = created["thread"].as_i64().unwrap();
        let (status, board) = call(erisi(), Method::GET, "/status", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(board["cards"].as_array().unwrap().len(), 1);
        assert_eq!(board["cards"][0]["agent"], "tsugumi-lab");
        assert_eq!(board["cards"][0]["stale"], false);
        assert_eq!(board["cards"][0]["lines"][0]["level"], "warn");
        assert_eq!(board["asks"][0]["asked"], "tsugumi-minecraft");
        assert_eq!(board["asks"][0]["author"], "tsugumi-lab");
        assert_eq!(board["waiting"][0]["id"], thread);
        assert_eq!(board["due"][0]["id"], thread);
        let ask = created["post"].as_i64().unwrap();
        call(
            erisi(),
            Method::POST,
            &format!("/threads/{thread}/posts"),
            Some(json!({"body": "survival", "reply_to": ask})),
        )
        .await;
        let (_, board) = call(erisi(), Method::GET, "/status", None).await;
        assert!(board["asks"].as_array().unwrap().is_empty());

        // An old card is stale; a deleted one is gone, and only its writer can delete it.
        state
            .with_board(|board| {
                board.conn.execute("UPDATE status SET updated = updated - 1000", [])?;
                Ok(())
            })
            .await
            .unwrap();
        let (_, board) = call(erisi(), Method::GET, "/status", None).await;
        assert_eq!(board["cards"][0]["stale"], true);
        let (status, _) = call(erisi(), Method::DELETE, "/status/lab", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(lab(), Method::DELETE, "/status/lab", None).await;
        assert_eq!(status, StatusCode::OK);
        let (_, board) = call(erisi(), Method::GET, "/status", None).await;
        assert!(board["cards"].as_array().unwrap().is_empty());
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
    async fn only_the_poller_writes_the_archive() {
        let state = state();
        let batch = json!({"messages": [{
            "id": 1555567596084924429_i64, "channel": 1, "author": "baughn",
            "author_kind": "human", "created": 1759400000, "content": "Not raw, but yes",
            "url": "https://discord.com/channels/1/1/1555567596084924429"}]});
        let (status, _) = call(
            app(&state, "tsugumi-lab"),
            Method::POST,
            "/discord",
            Some(batch.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, result) = call(
            app(&state, "discord"),
            Method::POST,
            "/discord",
            Some(batch),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["inserted"], 1);
        let (_, context) = call(
            app(&state, "tsugumi-lab"),
            Method::GET,
            "/discord/1555567596084924429",
            None,
        )
        .await;
        assert_eq!(context["messages"][0]["content"], "Not raw, but yes");
        let (_, cursor) = call(
            app(&state, "discord"),
            Method::GET,
            "/discord/cursor?channel=1",
            None,
        )
        .await;
        assert_eq!(cursor["last"], 1555567596084924429_i64);
    }

    #[tokio::test]
    async fn todos_create_list_and_validate() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let todo = |body: Value| call(lab(), Method::POST, "/todos", Some(body));

        let (status, later) = todo(json!({"title": "  Write the report  "})).await;
        assert_eq!(status, StatusCode::OK, "{later}");
        assert_eq!(
            later,
            json!({"id": later["id"], "title": "Write the report", "assignee": "tsugumi-lab",
                   "created_by": "tsugumi-lab", "created": later["created"], "due": null,
                   "thread": null, "status": "open", "closed_by": null, "closed": null,
                   "note": null})
        );
        assert!(later["created"].as_i64().unwrap() > 0);
        let (_, soon) = todo(json!({"title": "Approve", "assignee": "baughn",
                                    "due": "2026-11-01"})).await;
        let (_, sooner) = todo(json!({"title": "Restart", "assignee": "tsugumi-minecraft",
                                      "due": "2026-10-25"})).await;
        assert_eq!(soon["created_by"], "tsugumi-lab");

        // Soonest due first, undated last.
        let (status, list) = call(lab(), Method::GET, "/todos", None).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        let ids: Vec<&Value> = list["todos"].as_array().unwrap().iter().map(|t| &t["id"]).collect();
        assert_eq!(ids, [&sooner["id"], &soon["id"], &later["id"]]);
        let (_, list) = call(lab(), Method::GET, "/todos?assignee=baughn", None).await;
        assert_eq!(list["todos"].as_array().unwrap().len(), 1);
        assert_eq!(list["todos"][0]["id"], soon["id"]);
        let (_, list) = call(lab(), Method::GET, "/todos?status=done", None).await;
        assert_eq!(list, json!({"todos": []}));
        let (_, list) = call(lab(), Method::GET, "/todos?status=all&assignee=", None).await;
        assert_eq!(list["todos"].as_array().unwrap().len(), 3);

        // The caller's open todos in its briefing; all open todos on the dashboard.
        let (_, brief) = call(lab(), Method::GET, "/briefing", None).await;
        assert_eq!(brief["todos"].as_array().unwrap().len(), 1, "{brief}");
        assert_eq!(brief["todos"][0]["id"], later["id"]);
        let (_, brief) = call(lab(), Method::GET, "/briefing?agent=baughn", None).await;
        assert_eq!(brief["todos"][0]["id"], soon["id"]);
        let (_, dashboard) = call(lab(), Method::GET, "/status", None).await;
        assert_eq!(dashboard["todos"].as_array().unwrap().len(), 3);

        // Unknown assignees are refused with the valid names, also as a filter.
        let (status, error) = todo(json!({"title": "x", "assignee": "bob"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let message = error["error"].as_str().unwrap();
        assert!(message.contains("baughn") && message.contains("tsugumi-lab"), "{message}");
        let (status, _) = call(lab(), Method::GET, "/todos?assignee=bob", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Neither is a caller outside the roster a default assignee.
        let (status, _) = call(app(&state, "discord"), Method::POST, "/todos",
                               Some(json!({"title": "x"}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        for bad in [
            json!({"title": " "}),
            json!({"title": "two\nlines"}),
            json!({"title": "é".repeat(201)}),
            json!({"title": "x", "due": "tomorrow"}),
            json!({"title": "x", "due": "2026-13-01"}),
        ] {
            let (status, _) = todo(bad.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        }
        let (status, _) = todo(json!({"title": "é".repeat(200)})).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = todo(json!({"title": "x", "thread": 99})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(lab(), Method::GET, "/todos?status=bogus", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn todo_permissions_and_closing() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let erisi = || app(&state, "tsugumi-minecraft");
        let saya = || app(&state, "saya");
        let patch = |router: Router, id: &Value, body: Value| {
            let uri = format!("/todos/{id}");
            async move { call(router, Method::PATCH, &uri, Some(body)).await }
        };
        let (_, todo) = call(
            lab(),
            Method::POST,
            "/todos",
            Some(json!({"title": "Restart the server", "assignee": "tsugumi-minecraft",
                        "due": "2026-10-30"})),
        )
        .await;
        let id = &todo["id"];

        // Neither assignee nor creator.
        let (status, error) = patch(saya(), id, json!({"status": "done"})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(error["error"].as_str().unwrap().contains("tsugumi-minecraft"), "{error}");

        // The assignee closes it; the creator reopens it.
        let (status, done) = patch(erisi(), id, json!({"status": "done"})).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["status"], "done");
        assert_eq!(done["closed_by"], "tsugumi-minecraft");
        assert!(done["closed"].as_i64().is_some());
        let (_, open) = patch(lab(), id, json!({"status": "open"})).await;
        assert_eq!(open["status"], "open");
        assert_eq!(open["closed_by"], Value::Null);
        assert_eq!(open["closed"], Value::Null);

        // Dropping needs a note, given now or earlier; clearing it doesn't count.
        let (status, error) = patch(lab(), id, json!({"status": "dropped"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error["error"].as_str().unwrap().contains("note"), "{error}");
        let (status, _) = patch(lab(), id, json!({"status": "dropped", "note": " "})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (_, noted) = patch(lab(), id, json!({"note": "superseded by #9"})).await;
        assert_eq!(noted["note"], "superseded by #9");
        assert_eq!(noted["status"], "open");
        let (status, dropped) = patch(lab(), id, json!({"status": "dropped"})).await;
        assert_eq!(status, StatusCode::OK, "{dropped}");
        assert_eq!(dropped["closed_by"], "tsugumi-lab");
        let (status, _) = patch(lab(), id, json!({"note": ""})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        patch(lab(), id, json!({"status": "open"})).await;

        // Reassigned: the old assignee loses its say, the new one gains it.
        let (_, moved) = patch(lab(), id, json!({"assignee": "saya", "due": ""})).await;
        assert_eq!(moved["assignee"], "saya");
        assert_eq!(moved["created_by"], "tsugumi-lab");
        assert_eq!(moved["due"], Value::Null);
        let (status, _) = patch(erisi(), id, json!({"title": "x"})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = patch(saya(), id, json!({"assignee": "nobody"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, renamed) = patch(saya(), id, json!({"title": "Restart both servers"})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(renamed["title"], "Restart both servers");

        // Anyone may act on a human's todo.
        let (_, human) = call(
            lab(),
            Method::POST,
            "/todos",
            Some(json!({"title": "Approve the deploy", "assignee": "baughn"})),
        )
        .await;
        let (status, done) = patch(erisi(), &human["id"], json!({"status": "done",
                                                                  "note": "approved on Discord"})).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["closed_by"], "tsugumi-minecraft");
        assert_eq!(done["note"], "approved on Discord");

        // Threads link and unlink; an unknown thread is refused.
        let (_, created) = call(lab(), Method::POST, "/threads", Some(json!({"title": "T"}))).await;
        let thread = created["thread"].as_i64().unwrap();
        let (_, linked) = patch(saya(), id, json!({"thread": thread})).await;
        assert_eq!(linked["thread"], thread);
        let (_, unlinked) = patch(saya(), id, json!({"thread": null})).await;
        assert_eq!(unlinked["thread"], Value::Null);
        patch(saya(), id, json!({"thread": thread})).await;
        let (_, unlinked) = patch(saya(), id, json!({"thread": 0})).await;
        assert_eq!(unlinked["thread"], Value::Null);
        let (status, _) = patch(saya(), id, json!({"thread": 99})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = patch(saya(), &json!(99), json!({"status": "done"})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn open_todos_block_resolving() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let (_, created) = call(
            lab(),
            Method::POST,
            "/threads",
            Some(json!({"title": "Bug grab-bag", "summary": "Several bugs."})),
        )
        .await;
        let thread = created["thread"].as_i64().unwrap();
        let (_, fix) = call(lab(), Method::POST, "/todos",
                            Some(json!({"title": "fix X", "thread": thread}))).await;
        let (_, approve) = call(
            lab(),
            Method::POST,
            "/todos",
            Some(json!({"title": "approve Y", "thread": thread, "assignee": "baughn"})),
        )
        .await;
        let (_, list) = call(lab(), Method::GET, &format!("/todos?thread={thread}"), None).await;
        assert_eq!(list["todos"].as_array().unwrap().len(), 2);

        let thread_uri = format!("/threads/{thread}");
        let resolve = |body: Value| call(lab(), Method::PATCH, &thread_uri, Some(body));
        let (status, error) = resolve(json!({"status": "resolved", "summary": "All fixed."})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            error["error"],
            format!(
                "thread #{thread} still has open todos: #{} \"fix X\" (tsugumi-lab), \
                 #{} \"approve Y\" (baughn); close or drop them first",
                fix["id"], approve["id"]
            )
        );
        // Refused as a whole: the summary didn't change either.
        let (_, view) = call(lab(), Method::GET, &format!("/threads/{thread}"), None).await;
        assert_eq!(view["thread"]["summary"], "Several bugs.");
        assert_eq!(view["thread"]["status"], "open");
        assert_eq!(view["todos"].as_array().unwrap().len(), 2);

        // Other changes still go through.
        let (status, _) = resolve(json!({"status": "parked"})).await;
        assert_eq!(status, StatusCode::OK);

        call(lab(), Method::PATCH, &format!("/todos/{}", fix["id"]),
             Some(json!({"status": "done"}))).await;
        let (status, _) = resolve(json!({"status": "resolved"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        call(lab(), Method::PATCH, &format!("/todos/{}", approve["id"]),
             Some(json!({"status": "dropped", "note": "not needed"}))).await;
        let (status, resolved) = resolve(json!({"status": "resolved"})).await;
        assert_eq!(status, StatusCode::OK, "{resolved}");
        // The thread still lists its closed todos.
        let (_, view) = call(lab(), Method::GET, &format!("/threads/{thread}"), None).await;
        let statuses: Vec<&Value> = view["todos"].as_array().unwrap().iter().map(|t| &t["status"]).collect();
        assert_eq!(statuses, [&json!("done"), &json!("dropped")]);
    }

    #[tokio::test]
    async fn private_agents_todos() {
        let state = state().with_private(["tsugumi-sec".to_owned()]);
        let sec = || app(&state, "tsugumi-sec");
        let lab = || app(&state, "tsugumi-lab");
        let (_, public) = call(lab(), Method::POST, "/threads", Some(json!({"title": "Lab"}))).await;
        let public_thread = public["thread"].as_i64().unwrap();
        let (_, own) = call(sec(), Method::POST, "/threads", Some(json!({"title": "Watch"}))).await;
        let own_thread = own["thread"].as_i64().unwrap();
        let (_, public_todo) = call(lab(), Method::POST, "/todos",
                                    Some(json!({"title": "Lab work", "assignee": "baughn"}))).await;

        // Only to itself or a human, and only in its own threads.
        let (status, mine) = call(sec(), Method::POST, "/todos",
                                  Some(json!({"title": "Rescan", "thread": own_thread}))).await;
        assert_eq!(status, StatusCode::OK, "{mine}");
        let (status, for_baughn) = call(
            sec(),
            Method::POST,
            "/todos",
            Some(json!({"title": "Rotate the key", "assignee": "baughn", "due": "2026-10-30"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{for_baughn}");
        for body in [
            json!({"title": "x", "assignee": "tsugumi-lab"}),
            json!({"title": "x", "thread": public_thread}),
        ] {
            let (status, _) = call(sec(), Method::POST, "/todos", Some(body.clone())).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        }
        let mine_uri = format!("/todos/{}", mine["id"]);
        let (status, _) = call(sec(), Method::PATCH, &mine_uri,
                               Some(json!({"assignee": "tsugumi-lab"}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(sec(), Method::PATCH, &mine_uri,
                               Some(json!({"thread": public_thread}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // Nor does it change other agents' todos, even a human's.
        let (status, _) = call(sec(), Method::PATCH, &format!("/todos/{}", public_todo["id"]),
                               Some(json!({"status": "done"}))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(sec(), Method::PATCH, &mine_uri,
                               Some(json!({"status": "done"}))).await;
        assert_eq!(status, StatusCode::OK);

        // Invisible to other agents.
        let (_, list) = call(lab(), Method::GET, "/todos?assignee=baughn&status=all", None).await;
        assert_eq!(list["todos"].as_array().unwrap().len(), 1, "{list}");
        assert_eq!(list["todos"][0]["id"], public_todo["id"]);
        let (status, _) = call(lab(), Method::PATCH, &format!("/todos/{}", for_baughn["id"]),
                               Some(json!({"status": "done"}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (_, dashboard) = call(lab(), Method::GET, "/status", None).await;
        assert_eq!(dashboard["todos"].as_array().unwrap().len(), 1);
        let (_, brief) = call(lab(), Method::GET, "/briefing?agent=baughn", None).await;
        assert_eq!(brief["todos"].as_array().unwrap().len(), 1);
        let (status, _) = call(lab(), Method::GET, &format!("/todos?thread={own_thread}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The private agent sees its own and the public ones.
        let (_, list) = call(sec(), Method::GET, "/todos?status=all", None).await;
        assert_eq!(list["todos"].as_array().unwrap().len(), 3);
        let (_, view) = call(sec(), Method::GET, &format!("/threads/{own_thread}"), None).await;
        assert_eq!(view["todos"][0]["id"], mine["id"]);
        assert!(view["todos"][0].get("private_to").is_none());

        // Others may still assign it work.
        let (status, _) = call(lab(), Method::POST, "/todos",
                               Some(json!({"title": "Audit", "assignee": "tsugumi-sec"}))).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn pages_write_read_and_conflict() {
        let state = state();
        let lab = || app(&state, "tsugumi-lab");
        let saya = || app(&state, "saya");
        let put = |router: Router, body: Value| call(router, Method::PUT, "/page", Some(body));

        let (status, page) = put(
            lab(),
            json!({"slug": "mod/ea", "title": "Enchanted Armory", "body": "Kits keep length.",
                   "note": "first notes", "sources": ["#35"], "tags": ["mod"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(
            page,
            json!({"id": page["id"], "slug": "mod/ea", "title": "Enchanted Armory",
                   "description": "", "owner": "tsugumi-lab", "tags": ["mod"], "rev": 1,
                   "created": page["created"], "updated": page["created"],
                   "updated_by": "tsugumi-lab", "size": 17, "body": "Kits keep length.",
                   "note": "first notes", "sources": ["#35"], "current_rev": 1})
        );
        let (status, page) = put(
            saya(),
            json!({"slug": "mod/ea", "base_rev": 1, "body": "Kits keep length since 6041cd2f.",
                   "note": "fix shipped", "description": "the EA mod"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!((page["rev"].as_i64(), page["owner"].as_str()), (Some(2), Some("tsugumi-lab")));

        // A write against an old revision is refused with who changed it.
        let (status, error) =
            put(lab(), json!({"slug": "mod/ea", "base_rev": 1, "body": "x", "note": "y"})).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let message = error["error"].as_str().unwrap();
        assert!(message.starts_with("page mod/ea is at rev 2 (saya, "), "{message}");
        assert!(message.ends_with("not 1: read it again and reapply your change"), "{message}");
        let (status, _) = put(lab(), json!({"slug": "mod/ea", "title": "t", "body": "x"})).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, error) = put(lab(), json!({"slug": "Mod/EA", "title": "t", "body": "x"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error["error"].as_str().unwrap().contains("segments"), "{error}");

        let (status, old) = call(lab(), Method::GET, "/page?slug=mod/ea&rev=1", None).await;
        assert_eq!(status, StatusCode::OK, "{old}");
        assert_eq!((old["rev"].as_i64(), old["current_rev"].as_i64()), (Some(1), Some(2)));
        assert_eq!(old["body"], "Kits keep length.");
        let (_, current) = call(lab(), Method::GET, "/page?slug=mod%2Fea", None).await;
        assert_eq!(current["description"], "the EA mod");
        for uri in ["/page?slug=mod/ea&rev=3", "/page?slug=mod/none", "/page/revisions?slug=x"] {
            let (status, _) = call(lab(), Method::GET, uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (_, history) = call(lab(), Method::GET, "/page/revisions?slug=mod/ea", None).await;
        assert_eq!(history["slug"], "mod/ea");
        assert_eq!(history["revisions"][0]["rev"], 2);
        assert_eq!(history["revisions"][0]["note"], "fix shipped");
        assert_eq!(history["revisions"][1]["sources"], json!(["#35"]));
        assert!(history["revisions"][0].get("body").is_none());

        put(lab(), json!({"slug": "server/incognito", "title": "Incognito", "body": "x"})).await;
        let (_, list) = call(lab(), Method::GET, "/pages?prefix=mod&owner=", None).await;
        assert_eq!(list["pages"].as_array().unwrap().len(), 1, "{list}");
        assert_eq!(list["pages"][0]["updated_by"], "saya");
        assert!(list["pages"][0].get("body").is_none());
        let (_, list) = call(lab(), Method::GET, "/pages?tag=mod", None).await;
        assert_eq!(list["pages"][0]["slug"], "mod/ea");
        let (_, list) = call(lab(), Method::GET, "/pages", None).await;
        assert_eq!(list["pages"].as_array().unwrap().len(), 2);

        let (_, hits) = call(lab(), Method::GET, "/search?q=kits&kind=page", None).await;
        assert_eq!(hits[0]["slug"], "mod/ea", "{hits}");
        assert_eq!(hits[0]["thread_title"], "Enchanted Armory");
        let (status, _) = call(lab(), Method::GET, "/search?q=kits&kind=pages", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn private_pages() {
        let state = state().with_private(["tsugumi-sec".to_owned()]);
        let sec = || app(&state, "tsugumi-sec");
        let lab = || app(&state, "tsugumi-lab");
        let put = |router: Router, body: Value| call(router, Method::PUT, "/page", Some(body));
        let (status, page) =
            put(sec(), json!({"slug": "sec/watch", "title": "Watch", "body": "zebra secret"}))
                .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["private_to"], "tsugumi-sec");
        let (status, _) =
            put(lab(), json!({"slug": "lab/notes", "title": "Lab", "body": "zebra public"})).await;
        assert_eq!(status, StatusCode::OK);

        // Hidden from other agents, as if missing; but its slug is taken.
        for uri in ["/page?slug=sec/watch", "/page/revisions?slug=sec/watch"] {
            let (status, _) = call(lab(), Method::GET, uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
        let (_, list) = call(lab(), Method::GET, "/pages", None).await;
        assert_eq!(list["pages"].as_array().unwrap().len(), 1);
        let (_, hits) = call(lab(), Method::GET, "/search?q=zebra", None).await;
        assert_eq!(hits.as_array().unwrap().len(), 1, "{hits}");
        assert_eq!(hits[0]["slug"], "lab/notes");
        let (status, error) =
            put(lab(), json!({"slug": "sec/watch", "title": "Mine", "body": "x"})).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(error["error"], "slug sec/watch is taken");
        let (status, _) = put(lab(), json!({"slug": "sec/watch", "base_rev": 1, "body": "x"})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The private agent sees everything but writes only its own pages.
        let (_, list) = call(sec(), Method::GET, "/pages", None).await;
        assert_eq!(list["pages"].as_array().unwrap().len(), 2);
        let (_, hits) = call(sec(), Method::GET, "/search?q=zebra", None).await;
        assert_eq!(hits.as_array().unwrap().len(), 2);
        let (status, _) = put(sec(), json!({"slug": "lab/notes", "base_rev": 1, "body": "x"})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, page) = put(sec(), json!({"slug": "sec/watch", "base_rev": 1, "body": "y"})).await;
        assert_eq!(status, StatusCode::OK, "{page}");
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
