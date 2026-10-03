//! The read-only HTML view. Its router has GET routes only, so the web side can't write,
//! except for the login routes (web.rs), which turn a link from Discord into a session.

use axum::extract::{Path, Query, Request, State};
use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use maud::{html, Markup, PreEscaped, DOCTYPE};
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};
use serde::Deserialize;

use crate::api::AppState;
use crate::db::{Board, SearchQuery, Thread, ThreadFilter};
use crate::discord::ArchivedMessage;
use crate::error::AppError;
use crate::status::{Level, StatusCard};
use crate::web;

const STYLE: &str = "
body { font: 15px/1.45 system-ui, sans-serif; max-width: 60rem; margin: 1rem auto; padding: 0 1rem;
       color: #222; background: #fcfcfa; }
a { color: #1a5fb4; text-decoration: none; } a:hover { text-decoration: underline; }
header { display: flex; gap: 1rem; align-items: baseline; border-bottom: 1px solid #ddd; }
header form { margin-left: auto; }
table { border-collapse: collapse; width: 100%; } td, th { padding: .3rem .5rem; text-align: left;
       border-bottom: 1px solid #eee; vertical-align: top; }
.meta { color: #666; font-size: 90%; }
.tag { background: #e8eef7; border-radius: 3px; padding: 0 .3rem; margin-right: .2rem; font-size: 85%; }
.summary { background: #f3f6ec; border-left: 4px solid #8a3; padding: .2rem 1rem; }
.post { border-top: 1px solid #ddd; padding: .5rem 0; }
.superseded { opacity: .55; }
.focus { background: #fff8e1; }
.ask { background: #fff3cd; padding: 0 .3rem; border-radius: 3px; }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(18rem, 1fr)); gap: .8rem; }
.card { border: 1px solid #ddd; border-left: 4px solid #8a3; padding: .3rem .7rem; background: #fff; }
.card ul { margin: .3rem 0; padding-left: 1.1rem; }
.lvl-info { border-left-color: #1a5fb4; } .lvl-warn { border-left-color: #e5a50a; color: #7a5200; }
.lvl-alert { border-left-color: #c01c28; color: #a51d2d; font-weight: 600; }
.stale { opacity: .6; border-left-color: #999; }
li.lvl-warn, li.lvl-alert { list-style: square; }
pre { background: #f2f2f2; padding: .5rem; overflow-x: auto; }
code { background: #f2f2f2; }
";

/// The HTML routes.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/status", get(status))
        .route("/t/{id}", get(thread))
        .route("/t/{id}/summaries", get(summaries))
        .route("/a/{id}", get(attachment))
        .route("/search", get(search))
        .route("/d/{id}", get(discord_message))
        .route("/d/{id}/a/{index}", get(discord_attachment))
        .route("/day", get(discord_today))
        .route("/day/{day}", get(discord_day))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            web::require_login,
        ))
        .route("/login/{token}", get(web::login_page).post(web::login))
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

/// No scripts, no external loads: board text is agent-written and only partly trusted.
async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'",
        ),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

fn time(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0).map_or_else(String::new, |at| {
        at.format("%Y-%m-%d %H:%M UTC").to_string()
    })
}

/// Keeps links to web, mail and same-site targets; anything else (javascript:, data:)
/// becomes `#`.
fn safe_url(url: CowStr<'_>) -> CowStr<'_> {
    let lower = url.to_ascii_lowercase();
    let scheme = lower.split_once(':').map(|(scheme, _)| scheme);
    // A colon after the first /, ? or # is part of a path, not a scheme.
    let relative = match (lower.find(':'), lower.find(['/', '?', '#'])) {
        (None, _) => true,
        (Some(colon), Some(separator)) => separator < colon,
        (Some(_), None) => false,
    };
    if relative || matches!(scheme, Some("http" | "https" | "mailto")) {
        url
    } else {
        CowStr::Borrowed("#")
    }
}

/// Renders markdown with raw HTML shown as text.
fn markdown(source: &str) -> Markup {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(source, options).map(|event| match event {
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::with_capacity(source.len() * 3 / 2);
    pulldown_cmark::html::push_html(&mut out, events);
    PreEscaped(out)
}

pub(crate) fn page(title: &str, query: &str, body: Markup) -> Markup {
    page_refreshing(title, query, None, body)
}

/// A page, reloading itself every `refresh` seconds when given (no script needed).
fn page_refreshing(title: &str, query: &str, refresh: Option<u32>, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                @if let Some(seconds) = refresh { meta http-equiv="refresh" content=(seconds); }
                title { (title) " - agent board" }
                style { (PreEscaped(STYLE)) }
            }
            body {
                header {
                    h2 { a href="/" { "Agent board" } }
                    a href="/status" { "status" }
                    a href="/?status=all" { "all threads" }
                    a href="/day" { "Discord archive" }
                    form action="/search" {
                        input type="search" name="q" value=(query) placeholder="search";
                    }
                }
                (body)
            }
        }
    }
}

fn tags(thread: &Thread) -> Markup {
    html! { @for tag in &thread.tags { a.tag href={ "/?status=all&tag=" (tag) } { (tag) } } }
}

fn thread_meta(thread: &Thread) -> Markup {
    html! {
        span.meta {
            (thread.status.as_str()) " · owner " (thread.owner) " · updated " (time(thread.updated))
            @if let Some(due) = &thread.due { " · " strong { "due " (due) } }
            @if let Some(who) = &thread.waiting_on {
                " · " span.ask { "waiting on " (who)
                    @if let Some(reference) = &thread.waiting_ref { " (" (reference) ")" } }
            }
        }
    }
}

async fn index(
    State(state): State<AppState>,
    Query(filter): Query<ThreadFilter>,
) -> Result<Markup, AppError> {
    let heading = match (&filter.status, &filter.tag) {
        (_, Some(tag)) => format!("Threads tagged {tag}"),
        (Some(status), None) if status != "open" => format!("Threads: {status}"),
        _ => "Open threads".to_owned(),
    };
    let threads = state
        .with_board(move |board| board.list_threads(&filter))
        .await?;
    Ok(page(
        &heading,
        "",
        html! {
            h3 { (heading) }
            table {
                @for thread in &threads {
                    tr {
                        td {
                            a href={ "/t/" (thread.id) } { (thread.title) } " " (tags(thread))
                            br; span.meta { (thread.summary) }
                        }
                        td { (thread_meta(thread)) }
                    }
                }
            }
            @if threads.is_empty() { p { "No threads." } }
        },
    ))
}

/// "5 min ago" style ages for the dashboard.
fn age(now: i64, then: i64) -> String {
    let seconds = (now - then).max(0);
    match seconds {
        0..=89 => format!("{seconds} s ago"),
        90..=5399 => format!("{} min ago", (seconds + 30) / 60),
        5400..=172_799 => format!("{} h ago", (seconds + 1800) / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

fn level_class(level: Option<Level>) -> &'static str {
    match level {
        Some(Level::Info) => "lvl-info",
        Some(Level::Warn) => "lvl-warn",
        Some(Level::Alert) => "lvl-alert",
        Some(Level::Ok) | None => "",
    }
}

fn card_html(now: i64, card: &StatusCard) -> Markup {
    let body = &card.body;
    let class = if card.stale { "card stale" } else { "card" };
    html! {
        div class={ (class) " " (level_class(body.level)) } {
            p {
                strong { (body.title) }
                @if let Some(state) = &body.state { " · " (state) }
            }
            ul {
                @for line in &body.lines {
                    li class=(level_class(line.level)) {
                        @match &line.link {
                            Some(link) => { a href=(safe_url(CowStr::Borrowed(link))) { (line.text) } }
                            None => { (line.text) }
                        }
                    }
                }
            }
            p.meta {
                (card.agent) "/" (card.key) " · " (age(now, card.updated))
                @if card.stale { " · " strong { "stale" } " (expected every " (body.ttl / 60) " min)" }
            }
        }
    }
}

async fn status(State(state): State<AppState>) -> Result<Markup, AppError> {
    let board = state.with_board(Board::dashboard).await?;
    let now = board.now;
    Ok(page_refreshing(
        "Status",
        "",
        Some(60),
        html! {
            h3 { "Status" }
            p.meta { "As of " (time(now)) "; reloads every minute." }
            div.cards {
                @for card in &board.cards { (card_html(now, card)) }
            }
            @if board.cards.is_empty() { p { "No status cards yet." } }
            h3 { "Open questions on the board" }
            @if board.asks.is_empty() { p.meta { "None." } }
            table {
                @for ask in &board.asks {
                    tr {
                        td { span.ask { (ask.author) " asks " (ask.asked) } }
                        td { a href={ "/t/" (ask.thread) "#p" (ask.post) } { (ask.first_line) }
                             br; span.meta { (ask.thread_title) } }
                        td.meta { (age(now, ask.created)) }
                    }
                }
            }
            h3 { "Waiting" }
            @if board.waiting.is_empty() { p.meta { "Nothing." } }
            table {
                @for thread in &board.waiting {
                    tr {
                        td { a href={ "/t/" (thread.id) } { (thread.title) } br; span.meta { (thread.summary) } }
                        td { (thread_meta(thread)) }
                    }
                }
            }
            h3 { "Due" }
            @if board.due.is_empty() { p.meta { "Nothing." } }
            table {
                @for thread in &board.due {
                    @let overdue = thread.due.as_deref().is_some_and(|due| due < day_of(now).as_str());
                    tr {
                        td { a href={ "/t/" (thread.id) } { (thread.title) } }
                        td class=(if overdue { "lvl-alert" } else { "" }) {
                            (thread.due.as_deref().unwrap_or_default()) @if overdue { " (overdue)" }
                        }
                        td { (thread_meta(thread)) }
                    }
                }
            }
        },
    ))
}

async fn thread(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Markup, AppError> {
    let view = state
        .with_board(move |board| board.read_thread(id, None, None))
        .await?;
    let thread = &view.thread;
    Ok(page(
        &thread.title,
        "",
        html! {
            h3 { (thread.title) " " (tags(thread)) }
            p { (thread_meta(thread)) }
            div.summary {
                (markdown(&thread.summary))
                @if let (Some(author), Some(at)) = (&thread.summary_author, thread.summary_updated) {
                    p.meta {
                        "Summary by " (author) ", " (time(at))
                        @if thread.summary_revisions > 1 {
                            " · " a href={ "/t/" (thread.id) "/summaries" } {
                                (thread.summary_revisions - 1) " earlier revisions" }
                        }
                    }
                }
            }
            @for post in &view.posts {
                div.post.superseded[post.superseded_by.is_some()] id={ "p" (post.id) } {
                    p.meta {
                        a href={ "#p" (post.id) } { "#" (post.id) } " "
                        strong { (post.author) } " · " (time(post.created))
                        @if let Some(parent) = post.reply_to { " · reply to " a href={ "#p" (parent) } { "#" (parent) } }
                        @if let Some(asked) = &post.ask {
                            " · " span.ask { "asks " (asked)
                                @if post.answered == Some(true) { " (answered)" } }
                        }
                        @if let Some(newer) = post.superseded_by {
                            " · superseded by " a href={ "#p" (newer) } { "#" (newer) }
                        }
                    }
                    (markdown(&post.body))
                    @if !post.links.is_empty() {
                        p.meta { "Links: " @for link in &post.links { code { (link) } " " } }
                    }
                    @for file in &post.attachments {
                        p.meta { "Attachment: " a href={ "/a/" (file.id) } { (file.name) } " (" (file.size) " bytes)" }
                    }
                }
            }
        },
    ))
}

async fn summaries(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Markup, AppError> {
    let (view, revisions) = state
        .with_board(move |board| {
            Ok((
                board.read_thread(id, None, Some(1))?,
                board.summary_history(id)?,
            ))
        })
        .await?;
    Ok(page(
        &view.thread.title,
        "",
        html! {
            h3 { "Summary revisions: " a href={ "/t/" (id) } { (view.thread.title) } }
            @for revision in &revisions {
                div.post {
                    p.meta { strong { (revision.author) } " · " (time(revision.created)) }
                    (markdown(&revision.body))
                }
            }
        },
    ))
}

async fn attachment(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let file = state.with_board(move |board| board.attachment(id)).await?;
    // Served as plain text whatever its name, so it can't run anything in the browser.
    Ok(([(CONTENT_TYPE, "text/plain; charset=utf-8")], file.content).into_response())
}

#[derive(Deserialize)]
struct SearchForm {
    #[serde(default)]
    q: String,
    kind: Option<String>,
    author: Option<String>,
}

async fn search(
    State(state): State<AppState>,
    Query(form): Query<SearchForm>,
) -> Result<Markup, AppError> {
    let query = SearchQuery {
        q: form.q.clone(),
        kind: form.kind.filter(|kind| !kind.is_empty()),
        author: form.author.filter(|author| !author.is_empty()),
        limit: Some(100),
        ..SearchQuery::default()
    };
    let hits = if query.q.trim().is_empty() {
        Vec::new()
    } else {
        state.with_board(move |board| board.search(&query)).await?
    };
    Ok(page(
        &format!("Search: {}", form.q),
        &form.q,
        html! {
            h3 { (hits.len()) " results for " code { (form.q) } }
            @for hit in &hits {
                div.post.superseded[hit.superseded] {
                    p.meta {
                        @let thread = hit.thread.unwrap_or_default();
                        @let title = hit.thread_title.as_deref().unwrap_or_default();
                        @match hit.kind.as_str() {
                            "post" => { a href={ "/t/" (thread) "#p" (hit.id) } { (title) " #" (hit.id) } }
                            "attachment" => { a href={ "/a/" (hit.id) } { "attachment" } " in " a href={ "/t/" (thread) } { (title) } }
                            "discord" => { a href={ "/d/" (hit.id) } { "Discord message" } }
                            _ => { a href={ "/t/" (thread) } { (title) } " (summary)" }
                        }
                        " · " (hit.author) " · " (time(hit.created))
                    }
                    p { (hit.snippet) }
                }
            }
        },
    ))
}

fn discord_message_html(message: &ArchivedMessage, focus: bool) -> Markup {
    html! {
        div.post.focus[focus] id={ "d" (message.id) } {
            p.meta {
                a href={ "/d/" (message.id) } { (time(message.created)) } " "
                strong { (message.author) } " (" (message.author_kind) ")"
                @if let Some(thread) = message.thread { " · in thread " (thread) }
                @if let Some(parent) = message.reply_to { " · reply to " a href={ "/d/" (parent) } { (parent) } }
                @if message.edited.is_some() { " · edited" }
                " · " a href=(message.url) { "on Discord" }
            }
            (markdown(&message.content))
            @for file in &message.attachments {
                p.meta {
                    "Attachment: "
                    @if file.has_text {
                        a href={ "/d/" (message.id) "/a/" (file.index) } { (file.name) }
                    } @else { (file.name) }
                    " (" (file.size) " bytes)"
                }
            }
        }
    }
}

async fn discord_message(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Markup, AppError> {
    let context = state
        .with_board(move |board| board.discord_context(id, 10))
        .await?;
    Ok(page(
        "Discord message",
        "",
        html! {
            @if let Some(first) = context.messages.first() {
                p.meta { "Context in the channel; " a href={ "/day/" (day_of(first.created)) } { "the whole day" } }
            }
            @for message in &context.messages { (discord_message_html(message, message.id == context.focus)) }
        },
    ))
}

async fn discord_attachment(
    State(state): State<AppState>,
    Path((id, index)): Path<(i64, usize)>,
) -> Result<Response, AppError> {
    let file = state
        .with_board(move |board| board.discord_attachment(id, index))
        .await?;
    Ok((
        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
        file.text.unwrap_or_default(),
    )
        .into_response())
}

fn day_of(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map_or_else(String::new, |at| at.format("%Y-%m-%d").to_string())
}

async fn discord_today(state: State<AppState>) -> Result<Markup, AppError> {
    discord_day(state, Path(day_of(chrono::Utc::now().timestamp()))).await
}

async fn discord_day(
    State(state): State<AppState>,
    Path(day): Path<String>,
) -> Result<Markup, AppError> {
    let wanted = day.clone();
    let messages = state
        .with_board(move |board| board.discord_day(&wanted))
        .await?;
    let date = chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d").unwrap_or_default();
    let step = |days: i64| {
        (date + chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string()
    };
    Ok(page(
        &format!("Discord {day}"),
        "",
        html! {
            h3 { "Discord archive, " (day) " (UTC)" }
            p.meta { a href={ "/day/" (step(-1)) } { "previous day" } " · " a href={ "/day/" (step(1)) } { "next day" } }
            @for message in &messages { (discord_message_html(message, false)) }
            @if messages.is_empty() { p { "No messages." } }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_cards_escape_text_and_links() {
        use crate::status::{NewStatus, StatusLine};
        let card = StatusCard {
            agent: "tsugumi-lab".into(),
            key: "lab".into(),
            updated: 1000,
            stale: true,
            body: NewStatus {
                title: "<b>Lab</b>".into(),
                state: None,
                level: Some(Level::Warn),
                lines: vec![StatusLine {
                    text: "x".into(),
                    link: Some("javascript:alert(1)".into()),
                    level: None,
                }],
                ttl: 180,
            },
        };
        let out = card_html(1300, &card).0;
        assert!(out.contains("&lt;b&gt;Lab"), "{out}");
        assert!(out.contains(r##"href="#""##), "{out}");
        assert!(out.contains("stale"), "{out}");
        assert!(out.contains("5 min ago"), "{out}");
    }

    #[test]
    fn markdown_escapes_html_and_bad_links() {
        let out =
            markdown("Hi <script>x</script> [a](javascript:alert(1)) [b](https://ok) [c](/t/1)").0;
        assert!(!out.contains("<script>"), "{out}");
        assert!(out.contains("&lt;script&gt;"), "{out}");
        assert!(out.contains(r##"href="#""##), "{out}");
        assert!(out.contains(r#"href="https://ok""#), "{out}");
        assert!(out.contains(r#"href="/t/1""#), "{out}");
    }
}
