pub mod candidate;
pub mod delegate;

use std::io;

use crate::api::client::post;
use crate::api::paths;
use crate::api::types::{json_line, BuildCandidate, Decision};
use crate::util::json_error;

use candidate::{read_hook_candidate, read_hook_settings};
use delegate::delegate_remote_build;

#[derive(Clone, Debug)]
pub struct HookConfig {
    pub endpoint: String,
    pub host: String,
    pub remote_host: String,
    pub remote_store_uri: String,
    pub remote_builder: String,
    pub nix_bin: String,
    pub verbosity: String,
}

/// Run the Nix build hook loop.
pub fn run_hook(cfg: HookConfig) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let settings = read_hook_settings(&mut stdin)?;

    loop {
        let Some(candidate) = read_hook_candidate(&mut stdin)? else {
            return Ok(());
        };

        let decision = request_decision(&cfg, &candidate).unwrap_or_else(|err| Decision {
            decision: "decline".to_string(),
            reason: format!("daemon unavailable: {err}"),
            store_uri: None,
            metrics: None,
        });

        if decision.decision != "accept" {
            eprintln!("# decline");
            continue;
        }

        return delegate_remote_build(&cfg, &settings, &candidate, &mut stdin);
    }
}

fn request_decision(cfg: &HookConfig, candidate: &BuildCandidate) -> io::Result<Decision> {
    let body = candidate_for_daemon(candidate, cfg);
    let response = post(&cfg.endpoint, paths::DECISION_BUILD_CANDIDATE, &body)?;
    serde_json::from_str(&response).map_err(json_error)
}

fn candidate_for_daemon(candidate: &BuildCandidate, cfg: &HookConfig) -> String {
    let body = BuildCandidate {
        am_willing: candidate.am_willing,
        needed_system: candidate.needed_system.clone(),
        drv_path: candidate.drv_path.clone(),
        required_features: candidate.required_features.clone(),
        pname: candidate.pname.clone(),
        remote_host: cfg.remote_host.clone(),
        remote_store_uri: cfg.remote_store_uri.clone(),
    };
    json_line(&body)
}
