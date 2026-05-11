//! Integration tests covering the build-observation lifecycle invariants
//! from SPEC §"Build observation lifecycle" and §"Test suite".
//!
//! The controller's public surface (`make_decision`, `record_finish`,
//! `watchdog_tick`, `open_state`) is enough to exercise all retirement
//! paths without bringing up real TCP listeners.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nbb::controller::{
    handle_hook_connection, make_decision, now_ms_u64, open_state, record_finish, watchdog_tick,
    ControllerConfig, ControllerState, TargetRuntime,
};
use nbb::inflight::{drv_filename, write_sentinel, Sentinel};
use nbb::persistence::admissions;
use nbb::protocol::frame::{read_frame_async, write_frame_async, Frame};
use nbb::protocol::handshake::perform_handshake_async;
use nbb::protocol::ops::{
    op, AdmissionFinish, BuildStatus, DecideCandidate, Decision, EventBuildFinish, TelemetryBody,
};
use nbb::scheduler::{SchedulerPolicy, Target};

const SYSTEM: &str = "x86_64-linux";

fn unique_subdir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "nbb-lifecycle-{label}-{}-{}",
        std::process::id(),
        nbb::util::now_ms()
    ))
}

fn target(name: &str, capacity: u32, is_local: bool) -> Target {
    Target {
        name: name.to_string(),
        tcp_addr: "127.0.0.1:65535".parse().unwrap(),
        store_uri: format!("ssh-ng://svein@{name}.local"),
        builder_line: format!("ssh-ng://svein@{name}.local x86_64-linux . 1 1 - - -"),
        capacity,
        speed_multiplier: 1.0,
        is_controller_host: is_local,
    }
}

fn config(data_dir: PathBuf, inflight_dir: PathBuf, hook_socket: PathBuf) -> ControllerConfig {
    ControllerConfig {
        system: SYSTEM.to_string(),
        data_dir,
        inflight_dir,
        hook_socket,
        targets: vec![target("tsugumi", 8, false)],
        poll_interval: Duration::from_millis(1000),
        policy: SchedulerPolicy {
            min_remote_mem_available_kb: 1_000_000,
            unknown_p95_ms: 60_000,
        },
        max_samples_per_pname: 200,
    }
}

fn fresh_target_runtime(state: &Arc<ControllerState>, name: &str) {
    let now = now_ms_u64();
    let rt = TargetRuntime {
        last_pong_ms: Some(now),
        last_telemetry: Some(TelemetryBody {
            mem_available_kb: 8_000_000,
            psi_memory_some_avg10: Some(0.0),
            nix_slots_active: 0,
            sampled_at_ms: now,
        }),
    };
    state
        .target_runtimes
        .lock()
        .unwrap()
        .insert(name.to_string(), rt);
}

fn candidate(drv: &str) -> DecideCandidate {
    DecideCandidate {
        drv_path: drv.to_string(),
        system: SYSTEM.to_string(),
        required_features: vec![],
        hook_pid: 11111,
    }
}

fn finish_event(drv: &str, pname: &str, duration_ms: Option<u64>, ts_ms: u64) -> EventBuildFinish {
    EventBuildFinish {
        drv_path: drv.to_string(),
        pname: pname.to_string(),
        host: "tsugumi".to_string(),
        ts_ms,
        duration_ms,
        status: BuildStatus::Success,
        out_paths: vec!["/nix/store/out".to_string()],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_decide_then_finish_writes_observation_and_retires_admission() {
    let data = unique_subdir("happy-data");
    let inflight = unique_subdir("happy-inflight");
    let sock = unique_subdir("happy-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight, sock))
        .await
        .unwrap();
    fresh_target_runtime(&state, "tsugumi");

    let decision = make_decision(&state, &candidate("/nix/store/aaa-foo-1.0.drv"))
        .await
        .unwrap();
    let Decision::Accept { target } = decision else {
        panic!("expected Accept, got {decision:?}");
    };
    assert_eq!(target.name, "tsugumi");

    // One admission row.
    {
        let conn = state.conn.lock().await;
        let rows = admissions::list(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].drv_path, "/nix/store/aaa-foo-1.0.drv");
    }

    let event = finish_event(
        "/nix/store/aaa-foo-1.0.drv",
        "foo",
        Some(5_000),
        now_ms_u64(),
    );
    record_finish(&state, event).await.unwrap();

    let conn = state.conn.lock().await;
    assert!(admissions::list(&conn).unwrap().is_empty());
    let observation_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM build_observations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(observation_count, 1);

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finish_without_duration_retires_admission_but_writes_no_observation() {
    let data = unique_subdir("nodur-data");
    let inflight = unique_subdir("nodur-inflight");
    let sock = unique_subdir("nodur-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight, sock))
        .await
        .unwrap();
    fresh_target_runtime(&state, "tsugumi");

    let _ = make_decision(&state, &candidate("/nix/store/bbb-foo.drv"))
        .await
        .unwrap();

    // Agent restart simulation: matching finish arrives with no duration.
    let event = finish_event("/nix/store/bbb-foo.drv", "foo", None, now_ms_u64());
    record_finish(&state, event).await.unwrap();

    let conn = state.conn.lock().await;
    assert!(admissions::list(&conn).unwrap().is_empty());
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM build_observations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_finish_is_idempotent() {
    let data = unique_subdir("dup-data");
    let inflight = unique_subdir("dup-inflight");
    let sock = unique_subdir("dup-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight, sock))
        .await
        .unwrap();
    fresh_target_runtime(&state, "tsugumi");

    let _ = make_decision(&state, &candidate("/nix/store/ccc-foo.drv"))
        .await
        .unwrap();

    let event = finish_event("/nix/store/ccc-foo.drv", "foo", Some(1_234), 5_000);
    record_finish(&state, event.clone()).await.unwrap();
    record_finish(&state, event).await.unwrap();

    let conn = state.conn.lock().await;
    let observation_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM build_observations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        observation_count, 1,
        "duplicate finish must not double-write"
    );

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchdog_retires_dead_pid_sentinel_within_one_tick() {
    let data = unique_subdir("sweep-data");
    let inflight = unique_subdir("sweep-inflight");
    let sock = unique_subdir("sweep-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight.clone(), sock))
        .await
        .unwrap();
    fresh_target_runtime(&state, "tsugumi");

    let drv = "/nix/store/ddd-foo.drv";
    let _ = make_decision(&state, &candidate(drv)).await.unwrap();

    // Write a sentinel for an impossible PID.
    let dead_pid = 0x7FFF_FFFE;
    write_sentinel(
        &inflight,
        &Sentinel {
            pid: dead_pid,
            drv_path: drv.to_string(),
            admitted_at_ms: now_ms_u64(),
            predicted_ms: 60_000,
        },
    )
    .unwrap();

    let sentinel_path = inflight.join(drv_filename(drv));
    assert!(sentinel_path.exists());

    watchdog_tick(&state).await.unwrap();

    let conn = state.conn.lock().await;
    assert!(admissions::list(&conn).unwrap().is_empty());
    drop(conn);
    assert!(!sentinel_path.exists(), "watchdog must unlink the sentinel");

    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&inflight);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchdog_wall_clock_ttl_retires_overdue_admissions() {
    let data = unique_subdir("ttl-data");
    let inflight = unique_subdir("ttl-inflight");
    let sock = unique_subdir("ttl-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight, sock))
        .await
        .unwrap();

    // Manually insert an admission far in the past.
    {
        let conn = state.conn.lock().await;
        admissions::record(&conn, "/nix/store/eee-foo.drv", "tsugumi", 0, 10_000).unwrap();
        assert_eq!(admissions::list(&conn).unwrap().len(), 1);
    }

    // TTL is max(predicted_ms × 2, 60_000) = 60_000 ms, admitted at t=0.
    // We're well past that by now.
    watchdog_tick(&state).await.unwrap();

    let conn = state.conn.lock().await;
    assert!(
        admissions::list(&conn).unwrap().is_empty(),
        "long-stop TTL must retire stale admissions"
    );

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn controller_restart_clears_admissions_table() {
    let data = unique_subdir("restart-data");
    let inflight = unique_subdir("restart-inflight");
    let sock = unique_subdir("restart-sock").join("decide.sock");

    let cfg = config(data.clone(), inflight, sock);

    let state = open_state(cfg.clone()).await.unwrap();
    fresh_target_runtime(&state, "tsugumi");
    let _ = make_decision(&state, &candidate("/nix/store/fff-foo.drv"))
        .await
        .unwrap();
    {
        let conn = state.conn.lock().await;
        assert_eq!(admissions::list(&conn).unwrap().len(), 1);
    }
    drop(state);

    // Re-open: open_state clears admissions table on startup.
    let state = open_state(cfg).await.unwrap();
    let conn = state.conn.lock().await;
    assert!(admissions::list(&conn).unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_reports_failure_via_admission_finish_retires_without_observation() {
    let data = unique_subdir("hookfail-data");
    let inflight = unique_subdir("hookfail-inflight");
    let sock = unique_subdir("hookfail-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight, sock))
        .await
        .unwrap();
    fresh_target_runtime(&state, "tsugumi");

    let drv = "/nix/store/ggg-foo.drv";
    let _ = make_decision(&state, &candidate(drv)).await.unwrap();

    // Simulate the hook reporting cancellation through the protocol over a
    // duplex stream rather than a real Unix socket.
    let (mut hook_end, controller_end) = tokio::io::duplex(8192);
    let server_state = Arc::clone(&state);
    let server =
        tokio::spawn(async move { handle_hook_connection(controller_end, server_state).await });

    // Hook side: handshake, then ADMISSION_FINISH frame, then close.
    perform_handshake_async(&mut hook_end).await.unwrap();
    let finish = AdmissionFinish {
        drv_path: drv.to_string(),
        status: BuildStatus::Cancelled,
    };
    write_frame_async(
        &mut hook_end,
        &Frame::with_body(op::ADMISSION_FINISH, &finish).unwrap(),
    )
    .await
    .unwrap();
    drop(hook_end);
    server.await.unwrap().unwrap();

    let conn = state.conn.lock().await;
    assert!(admissions::list(&conn).unwrap().is_empty());
    let observation_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM build_observations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(observation_count, 0);

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_local_records_admission_for_controller_host() {
    // Controller host (saya) is a target. With no remote target available,
    // saya wins, and the scheduler reports RouteLocal. The wire-level reply
    // to the hook is Decline, but an admission row is recorded for saya so
    // its queue_ms reflects the in-flight local build.
    let data = unique_subdir("routelocal-data");
    let inflight = unique_subdir("routelocal-inflight");
    let sock = unique_subdir("routelocal-sock").join("decide.sock");
    let mut cfg = config(data.clone(), inflight, sock);
    cfg.targets = vec![target("saya", 16, true)];
    let state = open_state(cfg).await.unwrap();
    fresh_target_runtime(&state, "saya");

    let drv = "/nix/store/iii-foo.drv";
    let decision = make_decision(&state, &candidate(drv)).await.unwrap();
    assert!(
        matches!(decision, Decision::Decline),
        "hook should see Decline, got {decision:?}"
    );

    // Admission was recorded for saya.
    {
        let conn = state.conn.lock().await;
        let rows = admissions::list(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].drv_path, drv);
        assert_eq!(rows[0].target_name, "saya");
    }

    // Local agent reports the finish; admission retires and observation row written.
    let mut event = finish_event(drv, "foo", Some(2_500), now_ms_u64());
    event.host = "saya".to_string();
    record_finish(&state, event).await.unwrap();

    let conn = state.conn.lock().await;
    assert!(admissions::list(&conn).unwrap().is_empty());
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM build_observations WHERE host = 'saya'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    let _ = std::fs::remove_dir_all(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_decide_round_trip_over_duplex() {
    let data = unique_subdir("decide-data");
    let inflight = unique_subdir("decide-inflight");
    let sock = unique_subdir("decide-sock").join("decide.sock");
    let state = open_state(config(data.clone(), inflight, sock))
        .await
        .unwrap();
    fresh_target_runtime(&state, "tsugumi");

    let (mut hook_end, controller_end) = tokio::io::duplex(8192);
    let server_state = Arc::clone(&state);
    let server =
        tokio::spawn(async move { handle_hook_connection(controller_end, server_state).await });

    perform_handshake_async(&mut hook_end).await.unwrap();
    let cand = candidate("/nix/store/hhh-foo.drv");
    write_frame_async(
        &mut hook_end,
        &Frame::with_body(op::DECIDE_CANDIDATE, &cand).unwrap(),
    )
    .await
    .unwrap();

    let reply = read_frame_async(&mut hook_end).await.unwrap();
    assert_eq!(reply.op_id, op::DECISION);
    let decision: Decision = reply.decode_body().unwrap();
    let Decision::Accept { target } = decision else {
        panic!("expected Accept, got {decision:?}");
    };
    assert_eq!(target.name, "tsugumi");

    drop(hook_end);
    server.await.unwrap().unwrap();

    let conn = state.conn.lock().await;
    assert_eq!(admissions::list(&conn).unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&data);
}
