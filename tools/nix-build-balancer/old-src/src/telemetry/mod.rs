pub mod local;
pub mod remote;

pub use local::{read_telemetry, TelemetryCache};
pub use remote::{poll_remotes, read_remote_telemetry, remote_package_stats};
