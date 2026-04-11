use anyhow::{Context, Result};
use std::process::Command;
use tracing::{info, warn};

use crate::config::FirewallRule;

const COMMENT_PREFIX: &str = "game-watcher:";
const CHAIN: &str = "nixos-fw";
const JUMP_TARGET: &str = "nixos-fw-accept";

/// A rule we've inserted, stored so we can remove it precisely.
#[derive(Debug, Clone)]
pub struct InsertedRule {
    pub cmd: &'static str, // "iptables" or "ip6tables"
    pub spec: Vec<String>, // argv *after* the chain name (no -I/-D, no chain)
}

/// Signature shared by the real iptables runner and test stubs.
pub trait IptablesRunner {
    fn run(&self, cmd: &str, op: &str, chain: &str, pos: Option<u32>, spec: &[String]) -> Result<()>;
}

/// Default runner that shells out to `iptables` / `ip6tables`.
pub struct ShellRunner;

impl IptablesRunner for ShellRunner {
    fn run(&self, cmd: &str, op: &str, chain: &str, pos: Option<u32>, spec: &[String]) -> Result<()> {
        run_iptables(cmd, op, chain, pos, spec)
    }
}

/// Apply all firewall rules for a given game. Returns the inserted rules
/// in the order they were inserted so they can be reverted.
pub fn apply(
    app_id: u32,
    game: &str,
    rules: &[FirewallRule],
    runner: &dyn IptablesRunner,
) -> Result<Vec<InsertedRule>> {
    let count = rules.len();
    info!(game, app_id, count, "applying {count} firewall rules for {game} ({app_id})");
    let mut inserted = Vec::new();
    for rule in rules {
        for &cmd in ipt_commands(rule) {
            let spec = build_spec(app_id, rule);
            runner
                .run(cmd, "-I", CHAIN, Some(1), &spec)
                .with_context(|| format!("inserting {cmd} rule for {game} ({app_id})"))?;
            let iface = rule.interface.as_deref().unwrap_or("*");
            let proto = rule.proto.as_str();
            let port = rule.port;
            info!(
                game,
                app_id,
                %cmd,
                proto,
                port,
                iface,
                "inserted {cmd} rule for {game}: {iface} {proto}/{port}"
            );
            inserted.push(InsertedRule { cmd, spec });
        }
    }
    Ok(inserted)
}

/// Revert a set of previously inserted rules.
pub fn revert(
    game: &str,
    app_id: u32,
    inserted: &[InsertedRule],
    runner: &dyn IptablesRunner,
) -> Result<()> {
    let count = inserted.len();
    info!(game, app_id, count, "reverting {count} firewall rules for {game} ({app_id})");
    let mut first_error: Option<anyhow::Error> = None;
    for rule in inserted {
        let spec_str = rule.spec.join(" ");
        if let Err(e) = runner.run(rule.cmd, "-D", CHAIN, None, &rule.spec) {
            warn!(
                game,
                app_id,
                cmd = %rule.cmd,
                spec = %spec_str,
                error = %e,
                "failed to delete {} rule for {game}: {spec_str} ({e})",
                rule.cmd
            );
            if first_error.is_none() {
                first_error = Some(e);
            }
        } else {
            info!(
                game,
                app_id,
                cmd = %rule.cmd,
                spec = %spec_str,
                "deleted {} rule for {game}: {spec_str}",
                rule.cmd
            );
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Parse `iptables -S nixos-fw` and `ip6tables -S nixos-fw`, deleting any
/// rule whose comment begins with `game-watcher:`. Called at startup to
/// recover from unclean shutdowns.
pub fn cleanup_stale() -> Result<usize> {
    let mut removed = 0;
    for cmd in ["iptables", "ip6tables"] {
        let output = Command::new(cmd)
            .args(["-S", CHAIN])
            .output()
            .with_context(|| format!("running {cmd} -S {CHAIN}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(%cmd, %stderr, "failed to list rules; skipping");
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if !line.contains(COMMENT_PREFIX) {
                continue;
            }
            // Line looks like:
            //   -A nixos-fw -i wg0 -p udp -m udp --dport 27015 -m comment --comment "game-watcher:544550" -j nixos-fw-accept
            // Convert -A <chain> to a -D invocation.
            let Some(rest) = line.strip_prefix("-A ") else {
                continue;
            };
            let Some(spec_str) = rest.strip_prefix(&format!("{CHAIN} ")) else {
                continue;
            };
            // Shell-style split that handles quoted "game-watcher:<id>".
            let Some(spec) = shell_split(spec_str) else {
                warn!(line, "failed to tokenize stale rule");
                continue;
            };
            if let Err(e) = run_iptables(cmd, "-D", CHAIN, None, &spec) {
                warn!(error = %e, line, "failed to delete stale {cmd} rule: {line} ({e})");
            } else {
                info!(%cmd, line, "deleted stale {cmd} rule: {line}");
                removed += 1;
            }
        }
    }
    if removed > 0 {
        info!(removed, "cleaned up {removed} stale game-watcher rules");
    }
    Ok(removed)
}

fn ipt_commands(rule: &FirewallRule) -> &'static [&'static str] {
    if rule.ipv6 {
        &["iptables", "ip6tables"]
    } else {
        &["iptables"]
    }
}

fn build_spec(app_id: u32, rule: &FirewallRule) -> Vec<String> {
    let mut spec: Vec<String> = Vec::new();
    if let Some(iface) = &rule.interface {
        spec.push("-i".into());
        spec.push(iface.clone());
    }
    spec.push("-p".into());
    spec.push(rule.proto.as_str().into());
    spec.push("--dport".into());
    spec.push(rule.port.to_string());
    spec.push("-m".into());
    spec.push("comment".into());
    spec.push("--comment".into());
    spec.push(format!("{COMMENT_PREFIX}{app_id}"));
    spec.push("-j".into());
    spec.push(JUMP_TARGET.into());
    spec
}

fn run_iptables(cmd: &str, op: &str, chain: &str, pos: Option<u32>, spec: &[String]) -> Result<()> {
    let mut c = Command::new(cmd);
    c.arg(op).arg(chain);
    if let Some(p) = pos {
        c.arg(p.to_string());
    }
    c.args(spec);
    let output = c.output().with_context(|| format!("running {cmd}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{cmd} {op} failed: {stderr}");
    }
    Ok(())
}

/// Minimal shell-style tokenizer: splits on whitespace but keeps double-quoted
/// strings intact (without the quotes). iptables -S emits comments as
/// `--comment "game-watcher:544550"`.
fn shell_split(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut started = false;
    for ch in s.chars() {
        if in_quotes {
            if ch == '"' {
                in_quotes = false;
            } else {
                cur.push(ch);
            }
            continue;
        }
        if ch == '"' {
            in_quotes = true;
            started = true;
            continue;
        }
        if ch.is_whitespace() {
            if started {
                out.push(std::mem::take(&mut cur));
                started = false;
            }
            continue;
        }
        started = true;
        cur.push(ch);
    }
    if in_quotes {
        return None;
    }
    if started {
        out.push(cur);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Proto;
    use std::cell::RefCell;
    use tracing_test::traced_test;

    #[test]
    fn shell_split_basic() {
        let got = shell_split(r#"-i wg0 -p udp --dport 27015 -m comment --comment "game-watcher:544550" -j nixos-fw-accept"#).unwrap();
        assert_eq!(
            got,
            vec![
                "-i",
                "wg0",
                "-p",
                "udp",
                "--dport",
                "27015",
                "-m",
                "comment",
                "--comment",
                "game-watcher:544550",
                "-j",
                "nixos-fw-accept",
            ]
        );
    }

    /// Captures every (cmd, op, chain, pos, spec) call so tests can both
    /// inspect and stub iptables behavior.
    struct MockRunner {
        calls: RefCell<Vec<(String, String, String, Option<u32>, Vec<String>)>>,
        fail: bool,
    }

    impl MockRunner {
        fn ok() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail: false,
            }
        }
        fn failing() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail: true,
            }
        }
    }

    impl IptablesRunner for MockRunner {
        fn run(
            &self,
            cmd: &str,
            op: &str,
            chain: &str,
            pos: Option<u32>,
            spec: &[String],
        ) -> Result<()> {
            self.calls.borrow_mut().push((
                cmd.into(),
                op.into(),
                chain.into(),
                pos,
                spec.to_vec(),
            ));
            if self.fail {
                anyhow::bail!("mock iptables failure");
            }
            Ok(())
        }
    }

    fn stationeers_rule() -> FirewallRule {
        FirewallRule {
            proto: Proto::Udp,
            port: 27015,
            interface: Some("wg0".into()),
            ipv6: true,
        }
    }

    #[traced_test]
    #[test]
    fn apply_logs_inserted_rules_with_game_name() {
        let runner = MockRunner::ok();
        let inserted = apply(544550, "stationeers", &[stationeers_rule()], &runner).unwrap();

        // Two inserts: iptables + ip6tables.
        assert_eq!(inserted.len(), 2);
        assert_eq!(runner.calls.borrow().len(), 2);

        // The summary line is self-describing.
        assert!(logs_contain("applying 1 firewall rules for stationeers (544550)"));

        // Per-rule insert log: each cmd produces one line including the
        // interface, protocol, and port directly in the message text.
        assert!(logs_contain("inserted iptables rule for stationeers: wg0 udp/27015"));
        assert!(logs_contain("inserted ip6tables rule for stationeers: wg0 udp/27015"));

        // Structured fields are still present (visible via `journalctl -o verbose`).
        assert!(logs_contain("app_id=544550"));
        assert!(logs_contain("port=27015"));
    }

    #[traced_test]
    #[test]
    fn revert_logs_deleted_rules_with_game_name() {
        let runner = MockRunner::ok();
        let inserted = apply(544550, "stationeers", &[stationeers_rule()], &runner).unwrap();
        revert("stationeers", 544550, &inserted, &runner).unwrap();

        // 2 from apply + 2 from revert.
        assert_eq!(runner.calls.borrow().len(), 4);

        assert!(logs_contain("reverting 2 firewall rules for stationeers (544550)"));
        assert!(logs_contain("deleted iptables rule for stationeers"));
        assert!(logs_contain("deleted ip6tables rule for stationeers"));
        // The deleted-rule message embeds the full argv so the journal shows
        // exactly what was removed.
        assert!(logs_contain("--dport 27015"));
        assert!(logs_contain("game-watcher:544550"));
    }

    #[traced_test]
    #[test]
    fn revert_logs_failure_when_runner_errors() {
        let ok_runner = MockRunner::ok();
        let inserted = apply(544550, "stationeers", &[stationeers_rule()], &ok_runner).unwrap();

        let failing = MockRunner::failing();
        let result = revert("stationeers", 544550, &inserted, &failing);
        assert!(result.is_err());

        assert!(logs_contain("failed to delete iptables rule for stationeers"));
        assert!(logs_contain("mock iptables failure"));
    }

    #[traced_test]
    #[test]
    fn revert_with_no_rules_still_logs_summary() {
        let runner = MockRunner::ok();
        revert("captain-of-industry", 1594320, &[], &runner).unwrap();
        // Even with zero rules, the summary line confirms revert was invoked.
        assert!(logs_contain(
            "reverting 0 firewall rules for captain-of-industry (1594320)"
        ));
    }

    #[test]
    fn build_spec_includes_comment_marker() {
        let rule = stationeers_rule();
        let spec = build_spec(544550, &rule);
        assert!(spec.iter().any(|s| s == "game-watcher:544550"));
        assert!(spec.iter().any(|s| s == "--comment"));
        assert!(spec.iter().any(|s| s == "wg0"));
        assert!(spec.iter().any(|s| s == "27015"));
    }
}
