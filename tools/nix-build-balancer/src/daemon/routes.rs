use axum::{
    extract::State,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde_json::json;

use crate::api::paths;
use crate::api::types::{BuildCandidate, BuildEvent, Decision, Telemetry};
use crate::persistence::{finish_admission, record_event, stats_json};
use crate::scheduler::{decide_build_candidate, log_scheduler_decision};
use crate::telemetry::read_telemetry;

use super::error::AppError;
use super::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(paths::HEALTH, get(health))
        .route(paths::TELEMETRY, get(telemetry))
        .route(paths::STATS, get(stats))
        .route(paths::EVENT_BUILD_START, post(event_build_start))
        .route(paths::EVENT_BUILD_FINISH, post(event_build_finish))
        .route(paths::EVENT_ADMISSION_FINISH, post(event_admission_finish))
        .route(
            paths::DECISION_BUILD_CANDIDATE,
            post(decision_build_candidate),
        )
        .with_state(state)
}

async fn health() -> Response {
    Json(json!({ "ok": true })).into_response()
}

async fn telemetry(State(state): State<AppState>) -> Result<Json<Telemetry>, AppError> {
    let host = state.config.host.clone();
    let telemetry = tokio::task::spawn_blocking(move || read_telemetry(&host))
        .await
        .map_err(|err| AppError::Internal(format!("telemetry task panic: {err}")))??;
    Ok(Json(telemetry))
}

async fn stats(State(state): State<AppState>) -> Result<Response, AppError> {
    let data_dir = state.config.data_dir.clone();
    let body = tokio::task::spawn_blocking(move || stats_json(&data_dir))
        .await
        .map_err(|err| AppError::Internal(format!("stats task panic: {err}")))??;
    Ok(([("content-type", "application/json")], body).into_response())
}

async fn event_build_start(
    State(state): State<AppState>,
    Json(mut event): Json<BuildEvent>,
) -> Result<Response, AppError> {
    event.kind = "start".to_string();
    record_event_blocking(state, event).await
}

async fn event_build_finish(
    State(state): State<AppState>,
    Json(mut event): Json<BuildEvent>,
) -> Result<Response, AppError> {
    event.kind = "finish".to_string();
    record_event_blocking(state, event).await
}

async fn record_event_blocking(state: AppState, event: BuildEvent) -> Result<Response, AppError> {
    let cfg = state.config.clone();
    tokio::task::spawn_blocking(move || record_event(&cfg, &event))
        .await
        .map_err(|err| AppError::Internal(format!("event task panic: {err}")))??;
    Ok(ok_json())
}

async fn event_admission_finish(
    State(state): State<AppState>,
    Json(event): Json<BuildEvent>,
) -> Result<Response, AppError> {
    let data_dir = state.config.data_dir.clone();
    tokio::task::spawn_blocking(move || finish_admission(&data_dir, &event.drv_path))
        .await
        .map_err(|err| AppError::Internal(format!("admission task panic: {err}")))??;
    Ok(ok_json())
}

async fn decision_build_candidate(
    State(state): State<AppState>,
    Json(mut candidate): Json<BuildCandidate>,
) -> Result<Json<Decision>, AppError> {
    if candidate.am_willing == 0 {
        candidate.am_willing = 1;
    }
    if candidate.pname.is_empty() {
        candidate.pname = crate::util::pname_from_drv(&candidate.drv_path);
    }

    let cfg = state.config.clone();
    let candidate_for_task = clone_candidate(&candidate);
    let decision =
        tokio::task::spawn_blocking(move || decide_build_candidate(&cfg, &candidate_for_task))
            .await
            .map_err(|err| AppError::Internal(format!("decision task panic: {err}")))??;

    log_scheduler_decision(&state.config, &candidate, &decision);
    Ok(Json(decision))
}

fn clone_candidate(candidate: &BuildCandidate) -> BuildCandidate {
    BuildCandidate {
        am_willing: candidate.am_willing,
        needed_system: candidate.needed_system.clone(),
        drv_path: candidate.drv_path.clone(),
        required_features: candidate.required_features.clone(),
        pname: candidate.pname.clone(),
        remote_host: candidate.remote_host.clone(),
        remote_store_uri: candidate.remote_store_uri.clone(),
    }
}

fn ok_json() -> Response {
    Json(json!({ "ok": true })).into_response()
}
