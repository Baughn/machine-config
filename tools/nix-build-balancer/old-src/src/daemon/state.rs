use std::sync::Arc;

use crate::config::Config;
use crate::telemetry::TelemetryCache;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub telemetry: TelemetryCache,
}

impl AppState {
    pub fn new(config: Config) -> std::io::Result<Self> {
        let telemetry = TelemetryCache::start(config.host.clone())?;
        Ok(Self {
            config: Arc::new(config),
            telemetry,
        })
    }
}
