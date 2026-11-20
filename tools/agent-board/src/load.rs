//! The obelisk's fast feed (Baughn, msg 1573366176179032166; LabAssistant, msg
//! 1573366294743621735): tsugumi's load btop-style, attributed by systemd cgroup, written to
//! `load.json` beside the obelisk document every [`EVERY`].
//!
//! Attribution goes by cgroup, not user: ErisiAgent and the live servers both run as
//! `minecraft`. Groups are found by name under /sys/fs/cgroup/system.slice:
//! `agent-bridge-<id>.service` (an agent, with its gradle and claude), `minecraft@<world>.service`
//! (a live server) and `<lab>.slice` holding `minecraft-lab*@*.service` (a lab's servers).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::obelisk::{sanitize, Config};

/// How often load.json is rewritten.
pub const EVERY: Duration = Duration::from_secs(2);
/// Samples of history per group and for the machine: 60 s.
const HISTORY: usize = 30;
const CGROUP_ROOT: &str = "/sys/fs/cgroup/system.slice";

#[derive(Clone, Debug, Serialize)]
pub struct LoadDoc {
    pub v: u32,
    pub t: i64,
    /// CPUs in the machine; group `cpu` values are in CPUs (2.5 = two and a half busy).
    pub ncpu: usize,
    /// Each core's busy fraction, 0..1.
    pub cores: Vec<f64>,
    /// The whole machine's busy fraction, last 60 s, oldest first.
    pub cpu_hist: Vec<f64>,
    pub mem_used_mib: u64,
    pub mem_total_mib: u64,
    /// IO pressure (some, avg10), 0..1.
    pub io: f64,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Group {
    /// `agent:<id>`, `lab:<slice>` or `world:<name>`.
    pub id: String,
    /// agent, lab or server.
    pub kind: &'static str,
    pub name: String,
    pub cpu: f64,
    pub mem_mib: u64,
    /// CPUs used, last 60 s, oldest first.
    pub hist: Vec<f64>,
    /// The process using the most CPU since the last sample (its command name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
}

/// What the sampler remembers between rounds.
#[derive(Default)]
pub struct Sampler {
    root: PathBuf,
    proc: PathBuf,
    last_at: Option<std::time::Instant>,
    /// Per cgroup: cpu.stat usage_usec at the last sample.
    usage: HashMap<String, u64>,
    /// Per pid: utime + stime (ticks) at the last sample.
    ticks: HashMap<u32, u64>,
    /// /proc/stat per core: (busy, total) jiffies.
    cores: Vec<(u64, u64)>,
    machine: (u64, u64),
    cpu_hist: VecDeque<f64>,
    hist: HashMap<String, VecDeque<f64>>,
}

/// A cgroup found under the root, with what it means.
struct Found {
    id: String,
    kind: &'static str,
    name: String,
    /// One cgroup, or for a lab every server unit in its slice (summed).
    paths: Vec<PathBuf>,
}

/// `mclab*.slice` directories under `dir`, nested ones included.
fn lab_slices(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with("mclab") && name.ends_with(".slice") {
            out.push(entry.path());
            lab_slices(&entry.path(), out);
        }
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn usage_usec(cgroup: &Path) -> Option<u64> {
    read(&cgroup.join("cpu.stat"))?
        .lines()
        .find_map(|line| line.strip_prefix("usage_usec ")?.trim().parse().ok())
}

/// Every pid in `cgroup` and below.
fn pids(cgroup: &Path, out: &mut Vec<u32>) {
    if let Some(text) = read(&cgroup.join("cgroup.procs")) {
        out.extend(
            text.lines()
                .filter_map(|line| line.trim().parse::<u32>().ok()),
        );
    }
    if let Ok(entries) = std::fs::read_dir(cgroup) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pids(&entry.path(), out);
            }
        }
    }
}

/// (command name, utime + stime) from /proc/<pid>/stat.
fn proc_ticks(proc: &Path, pid: u32) -> Option<(String, u64)> {
    let stat = read(&proc.join(pid.to_string()).join("stat"))?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_owned();
    let fields: Vec<&str> = stat.get(close + 2..)?.split_whitespace().collect();
    // After the comm: state is field 3, utime 14 and stime 15 (1-based in proc(5)).
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some((comm, utime + stime))
}

/// /proc/stat: the machine's and each core's (busy, total) jiffies.
/// (busy, total) jiffies.
type Jiffies = (u64, u64);

fn proc_stat(proc: &Path) -> Option<(Jiffies, Vec<Jiffies>)> {
    let text = read(&proc.join("stat"))?;
    let parse = |line: &str| -> Option<(u64, u64)> {
        let values: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|v| v.parse().ok())
            .collect();
        let total: u64 = values.iter().sum();
        // idle + iowait
        let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
        Some((total.saturating_sub(idle), total))
    };
    let mut machine = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        if line.starts_with("cpu ") {
            machine = parse(line);
        } else if line.starts_with("cpu") && line.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
        {
            cores.push(parse(line)?);
        }
    }
    Some((machine?, cores))
}

fn fraction(now: (u64, u64), before: (u64, u64)) -> f64 {
    let total = now.1.saturating_sub(before.1);
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let busy = now.0.saturating_sub(before.0) as f64 / total as f64;
    round(busy.clamp(0.0, 1.0))
}

fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn push(history: &mut VecDeque<f64>, value: f64) {
    if history.len() == HISTORY {
        history.pop_front();
    }
    history.push_back(value);
}

impl Sampler {
    #[must_use]
    pub fn new() -> Self {
        Self::with_roots(CGROUP_ROOT, "/proc")
    }

    #[must_use]
    pub fn with_roots(root: impl Into<PathBuf>, proc: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            proc: proc.into(),
            ..Self::default()
        }
    }

    fn find(&self, config: &Config) -> Vec<Found> {
        let names: HashMap<&str, &str> = config
            .agents
            .iter()
            .map(|agent| (agent.id.as_str(), agent.name.as_str()))
            .collect();
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return found;
        };
        let mut entries: Vec<_> = entries.flatten().map(|entry| entry.file_name()).collect();
        entries.sort();
        for entry in entries {
            let Some(unit) = entry.to_str() else { continue };
            let path = self.root.join(unit);
            if let Some(id) = unit
                .strip_prefix("agent-bridge-")
                .and_then(|rest| rest.strip_suffix(".service"))
            {
                // Only the obelisk's agents: other bridges aren't on it.
                if let Some(name) = names.get(id) {
                    found.push(Found {
                        id: format!("agent:{id}"),
                        kind: "agent",
                        name: (*name).to_owned(),
                        paths: vec![path],
                    });
                }
            } else if let Some(world) = unit
                .strip_prefix("minecraft@")
                .and_then(|rest| rest.strip_suffix(".service"))
            {
                found.push(Found {
                    id: format!("world:{world}"),
                    kind: "server",
                    name: world.to_owned(),
                    paths: vec![path],
                });
            }
        }
        // Lab slices sit beside system.slice; a dash nests a slice in another, so
        // mclab-assistant.slice is inside mclab.slice. A lab is the server units directly in
        // its own slice.
        if let Some(parent) = self.root.parent() {
            let mut slices = Vec::new();
            lab_slices(parent, &mut slices);
            slices.sort();
            for slice in slices {
                let Some(lab) = slice
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_suffix(".slice"))
                    .map(str::to_owned)
                else {
                    continue;
                };
                let mut units: Vec<PathBuf> = std::fs::read_dir(&slice)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "service"))
                    .collect();
                units.sort();
                // mclab is LabAgent's lab, mclab-assistant LabAssistant's.
                let owner = format!("tsugumi-lab{}", lab.trim_start_matches("mclab"));
                let name = names
                    .get(owner.as_str())
                    .map_or_else(|| lab.clone(), |name| format!("{name}'s lab"));
                found.push(Found {
                    id: format!("lab:{lab}"),
                    kind: "lab",
                    name,
                    paths: units,
                });
            }
        }
        found
    }

    /// One round: reads the counters and returns the document (the first round has no rates).
    pub fn sample(&mut self, config: &Config, t: i64) -> LoadDoc {
        let now = std::time::Instant::now();
        let elapsed = self.last_at.map(|at| now.duration_since(at).as_secs_f64());
        self.last_at = Some(now);

        let (machine, cores) = proc_stat(&self.proc).unwrap_or_default();
        let core_fractions: Vec<f64> = cores
            .iter()
            .enumerate()
            .map(|(index, core)| {
                self.cores
                    .get(index)
                    .map_or(0.0, |before| fraction(*core, *before))
            })
            .collect();
        let machine_fraction = fraction(machine, self.machine);
        if elapsed.is_some() {
            push(&mut self.cpu_hist, machine_fraction);
        }
        self.cores = cores;
        self.machine = machine;

        // Process ticks for the top-process pick: USER_HZ is 100 on Linux.
        let mut ticks = HashMap::new();
        let mut groups = Vec::new();
        let mut seen = Vec::new();
        for found in self.find(config) {
            let usages: Vec<u64> = found
                .paths
                .iter()
                .filter_map(|path| usage_usec(path))
                .collect();
            let usage = (!usages.is_empty()).then(|| usages.iter().sum::<u64>());
            #[allow(clippy::cast_precision_loss)]
            let cpu = match (usage, self.usage.get(&found.id), elapsed) {
                (Some(now), Some(before), Some(seconds)) if seconds > 0.0 => {
                    round(now.saturating_sub(*before) as f64 / 1e6 / seconds)
                }
                _ => 0.0,
            };
            if let Some(usage) = usage {
                self.usage.insert(found.id.clone(), usage);
            }
            let mem_mib = found
                .paths
                .iter()
                .filter_map(|path| {
                    read(&path.join("memory.current"))?
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
                .sum::<u64>()
                / (1024 * 1024);
            let mut group_pids = Vec::new();
            for path in &found.paths {
                pids(path, &mut group_pids);
            }
            let mut top: Option<(u64, String)> = None;
            for pid in group_pids {
                let Some((comm, total)) = proc_ticks(&self.proc, pid) else {
                    continue;
                };
                let delta = total.saturating_sub(self.ticks.get(&pid).copied().unwrap_or(total));
                ticks.insert(pid, total);
                if top.as_ref().is_none_or(|(best, _)| delta > *best) {
                    top = Some((delta, comm));
                }
            }
            let history = self.hist.entry(found.id.clone()).or_default();
            if elapsed.is_some() {
                push(history, cpu);
            }
            seen.push(found.id.clone());
            groups.push(Group {
                id: found.id,
                kind: found.kind,
                name: sanitize(&found.name, 40).unwrap_or_default(),
                cpu,
                mem_mib,
                hist: history.iter().copied().collect(),
                top: top.and_then(|(_, comm)| sanitize(&comm, 16)),
            });
        }
        self.ticks = ticks;
        self.hist.retain(|id, _| seen.contains(id));
        self.usage.retain(|id, _| seen.contains(id));

        let meminfo = read(&self.proc.join("meminfo")).unwrap_or_default();
        let field = |name: &str| -> u64 {
            meminfo
                .lines()
                .find(|line| line.starts_with(name))
                .and_then(|line| line.split_whitespace().nth(1)?.parse().ok())
                .unwrap_or(0)
        };
        let total = field("MemTotal:");
        let available = field("MemAvailable:");
        let io = read(&self.proc.join("pressure/io"))
            .and_then(|text| {
                let some = text.lines().find(|line| line.starts_with("some"))?;
                let avg = some
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix("avg10="))?;
                avg.parse::<f64>().ok()
            })
            .map_or(0.0, |percent| round((percent / 100.0).min(1.0)));
        LoadDoc {
            v: 1,
            t,
            ncpu: core_fractions.len(),
            cores: core_fractions,
            cpu_hist: self.cpu_hist.iter().copied().collect(),
            mem_used_mib: total.saturating_sub(available) / 1024,
            mem_total_mib: total / 1024,
            io,
            groups,
        }
    }
}

/// Writes load.json every [`EVERY`] next to the obelisk file.
pub async fn write_loop(config: Config, path: PathBuf) {
    let mut sampler = Sampler::new();
    let mut ticker = tokio::time::interval(EVERY);
    loop {
        ticker.tick().await;
        let doc = sampler.sample(&config, crate::db::now());
        let result = serde_json::to_vec(&doc)
            .map_err(anyhow::Error::from)
            .and_then(|json| crate::obelisk::write_atomic(&path, &json));
        if let Err(error) = result {
            tracing::warn!("obelisk load file {}: {error:#}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obelisk::Agent;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn samples_groups_by_cgroup() {
        let dir = tempfile::tempdir().unwrap();
        let cg = dir.path().join("cg");
        let system = cg.join("system.slice");
        let proc = dir.path().join("proc");
        let bridge = system.join("agent-bridge-tsugumi-lab.service");
        write(&bridge.join("cpu.stat"), "usage_usec 1000000\n");
        write(&bridge.join("memory.current"), "2147483648\n");
        write(&bridge.join("cgroup.procs"), "42\n");
        write(
            &system.join("agent-bridge-saya.service/cpu.stat"),
            "usage_usec 5\n",
        );
        write(
            &system.join("minecraft@incognito.service/cpu.stat"),
            "usage_usec 0\n",
        );
        write(
            &cg.join("mclab.slice/minecraft-lab@x.service/cgroup.procs"),
            "43\n",
        );
        write(
            &cg.join("mclab.slice/minecraft-lab@x.service/cpu.stat"),
            "usage_usec 0\n",
        );
        write(&cg.join("mclab.slice/mclab-assistant.slice/minecraft-lab-assistant@y.service/memory.current"), "1048576\n");
        write(
            &proc.join("42/stat"),
            "42 (java) S 1 1 1 0 -1 0 0 0 0 0 100 50 0 0\n",
        );
        write(
            &proc.join("43/stat"),
            "43 (java x) S 1 1 1 0 -1 0 0 0 0 0 5 5 0 0\n",
        );
        write(
            &proc.join("stat"),
            "cpu 10 0 10 80 0\ncpu0 5 0 5 40 0\ncpu1 5 0 5 40 0\n",
        );
        write(
            &proc.join("meminfo"),
            "MemTotal: 2048000 kB\nMemAvailable: 1024000 kB\n",
        );
        let config = Config {
            agents: vec![Agent {
                id: "tsugumi-lab".into(),
                name: "LabAgent".into(),
            }],
            file: None,
        };
        let mut sampler = Sampler::with_roots(&system, &proc);
        let first = sampler.sample(&config, 1);
        assert_eq!(first.ncpu, 2);
        let ids: Vec<&str> = first.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "agent:tsugumi-lab",
                "world:incognito",
                "lab:mclab",
                "lab:mclab-assistant"
            ]
        );
        assert_eq!(first.groups[3].mem_mib, 1);
        assert_eq!(first.groups[0].mem_mib, 2048);
        assert_eq!(first.groups[2].name, "LabAgent's lab");
        assert_eq!(first.mem_used_mib, 1000);

        write(&bridge.join("cpu.stat"), "usage_usec 3000000\n");
        write(
            &proc.join("42/stat"),
            "42 (java) S 1 1 1 0 -1 0 0 0 0 0 300 50 0 0\n",
        );
        write(
            &proc.join("stat"),
            "cpu 60 0 10 130 0\ncpu0 55 0 5 40 0\ncpu1 5 0 5 90 0\n",
        );
        let second = sampler.sample(&config, 3);
        assert!(second.groups[0].cpu > 0.0);
        assert_eq!(second.groups[0].hist.len(), 1);
        assert_eq!(second.groups[0].top.as_deref(), Some("java"));
        assert_eq!(second.groups[2].top.as_deref(), Some("java x"));
        assert!(
            (second.cores[0] - 1.0).abs() < 1e-9 && second.cores[1] == 0.0,
            "{:?}",
            second.cores
        );
    }
}
