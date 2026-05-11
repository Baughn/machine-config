use bincode::{Decode, Encode};

/// Operation IDs that fit in the 2-byte `op_id` field of a [`super::Frame`].
///
/// 0 is reserved (the source-tree handshake occupies the first 32 bytes of a
/// connection but is not modelled as a frame).
pub mod op {
    pub const AGENT_HELLO: u16 = 1;
    pub const TELEMETRY_GET: u16 = 2;
    pub const TELEMETRY: u16 = 3;
    pub const EVENT_BUILD_FINISH: u16 = 4;
    pub const PING: u16 = 5;
    pub const PONG: u16 = 6;
    pub const DECIDE_CANDIDATE: u16 = 7;
    pub const DECISION: u16 = 8;
    pub const ADMISSION_FINISH: u16 = 9;
}

/// Sent by an agent immediately after the handshake, identifying itself to
/// the controller.
#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct AgentHello {
    pub name: String,
    pub system: String,
    pub capacity: u32,
}

/// Body of a `TELEMETRY` frame — one telemetry snapshot from an agent.
#[derive(Encode, Decode, Clone, Debug, PartialEq)]
pub struct TelemetryBody {
    pub mem_available_kb: u64,
    pub psi_memory_some_avg10: Option<f64>,
    pub nix_slots_active: u32,
    pub sampled_at_ms: u64,
}

/// Build completion observation pushed from an agent to the controller. The
/// matching `Start` event lives only in the agent's in-memory map; when the
/// agent restarts between start and finish, `duration_ms` is `None` and the
/// controller retires the admission without writing an observation row.
#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct EventBuildFinish {
    pub drv_path: String,
    pub pname: String,
    pub host: String,
    pub ts_ms: u64,
    pub duration_ms: Option<u64>,
    pub status: BuildStatus,
    pub out_paths: Vec<String>,
}

#[derive(Encode, Decode, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuildStatus {
    Success,
    Failure,
    Cancelled,
}

impl BuildStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            BuildStatus::Success => "success",
            BuildStatus::Failure => "failure",
            BuildStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Request from `nbb-hook` (over the local Unix socket) asking the
/// controller whether to delegate a specific derivation.
#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct DecideCandidate {
    pub drv_path: String,
    pub system: String,
    pub required_features: Vec<String>,
    pub hook_pid: u32,
}

/// Response to [`DecideCandidate`].
#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Decline,
    Accept { target: AcceptTarget },
}

#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct AcceptTarget {
    pub name: String,
    pub store_uri: String,
    pub builder_line: String,
}

/// Sent by the hook on every exit path (success, failure, signal) and also
/// synthesised by the controller's watchdog when a sentinel PID is dead.
#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct AdmissionFinish {
    pub drv_path: String,
    pub status: BuildStatus,
}

/// On-disk spool event written by `nbb-event` and consumed by `nbb-agent`.
/// Same body schema for both start and finish; the agent matches starts
/// in memory and forwards `Finish` events to the controller as
/// [`EventBuildFinish`].
#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub enum SpoolEvent {
    Start {
        drv_path: String,
        pname: String,
        host: String,
        ts_ms: u64,
    },
    Finish {
        drv_path: String,
        pname: String,
        host: String,
        ts_ms: u64,
        status: BuildStatus,
        out_paths: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::frame::{bincode_config, Frame};

    fn round_trip<T: Encode + Decode<()> + PartialEq + std::fmt::Debug>(value: T, op_id: u16) {
        let frame = Frame::with_body(op_id, &value).unwrap();
        let decoded: T = frame.decode_body().unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn agent_hello_round_trip() {
        round_trip(
            AgentHello {
                name: "tsugumi".to_string(),
                system: "x86_64-linux".to_string(),
                capacity: 16,
            },
            op::AGENT_HELLO,
        );
    }

    #[test]
    fn telemetry_round_trip_with_psi() {
        round_trip(
            TelemetryBody {
                mem_available_kb: 12_345_678,
                psi_memory_some_avg10: Some(0.42),
                nix_slots_active: 7,
                sampled_at_ms: 1_700_000_000_000,
            },
            op::TELEMETRY,
        );
    }

    #[test]
    fn telemetry_round_trip_without_psi() {
        round_trip(
            TelemetryBody {
                mem_available_kb: 0,
                psi_memory_some_avg10: None,
                nix_slots_active: 0,
                sampled_at_ms: 0,
            },
            op::TELEMETRY,
        );
    }

    #[test]
    fn event_build_finish_with_duration_round_trip() {
        round_trip(
            EventBuildFinish {
                drv_path: "/nix/store/abc-foo.drv".to_string(),
                pname: "foo".to_string(),
                host: "tsugumi".to_string(),
                ts_ms: 1,
                duration_ms: Some(5_000),
                status: BuildStatus::Success,
                out_paths: vec!["/nix/store/xyz-foo".to_string()],
            },
            op::EVENT_BUILD_FINISH,
        );
    }

    #[test]
    fn event_build_finish_without_duration_round_trip() {
        round_trip(
            EventBuildFinish {
                drv_path: "/nix/store/abc-foo.drv".to_string(),
                pname: "foo".to_string(),
                host: "tsugumi".to_string(),
                ts_ms: 1,
                duration_ms: None,
                status: BuildStatus::Cancelled,
                out_paths: vec![],
            },
            op::EVENT_BUILD_FINISH,
        );
    }

    #[test]
    fn decision_accept_and_decline_round_trip() {
        round_trip(Decision::Decline, op::DECISION);
        round_trip(
            Decision::Accept {
                target: AcceptTarget {
                    name: "tsugumi".to_string(),
                    store_uri: "ssh-ng://svein@tsugumi.local".to_string(),
                    builder_line: "ssh-ng://svein@tsugumi.local x86_64-linux ... 16 1 nixos-test,kvm,big-parallel - -".to_string(),
                },
            },
            op::DECISION,
        );
    }

    #[test]
    fn spool_event_round_trip_both_variants() {
        round_trip(
            SpoolEvent::Start {
                drv_path: "/nix/store/abc-foo.drv".to_string(),
                pname: "foo".to_string(),
                host: "tsugumi".to_string(),
                ts_ms: 100,
            },
            op::EVENT_BUILD_FINISH, // op_id unused in spool tests; reuse a constant
        );
        round_trip(
            SpoolEvent::Finish {
                drv_path: "/nix/store/abc-foo.drv".to_string(),
                pname: "foo".to_string(),
                host: "tsugumi".to_string(),
                ts_ms: 200,
                status: BuildStatus::Failure,
                out_paths: vec![],
            },
            op::EVENT_BUILD_FINISH,
        );
    }

    #[test]
    fn admission_finish_round_trip() {
        round_trip(
            AdmissionFinish {
                drv_path: "/nix/store/abc-foo.drv".to_string(),
                status: BuildStatus::Success,
            },
            op::ADMISSION_FINISH,
        );
    }

    #[test]
    fn bincode_config_uses_varint_encoding() {
        // Sanity-check that the standard config produces compact output for
        // small integers — this is the property we care about, not the exact
        // bytes.
        let small = 1u64;
        let large = u64::MAX;
        let small_bytes = bincode::encode_to_vec(small, bincode_config()).unwrap();
        let large_bytes = bincode::encode_to_vec(large, bincode_config()).unwrap();
        assert!(
            small_bytes.len() < large_bytes.len(),
            "varint should make small ints smaller than large ones"
        );
    }
}
