//! Terminal progress display for the build phase.
//!
//! A render thread redraws a compact status tree on stderr at a capped rate,
//! snapshotting the shared [`Model`]. Frames are disposable: the model is
//! always current, and a slow terminal only delays the display, never the
//! parser feeding the model.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::log_model::Model;

const FRAME_INTERVAL: Duration = Duration::from_millis(80);
const MAX_BUILD_ROWS: usize = 12;
const SPINNER: &[char] = &['|', '/', '-', '\\'];

const CURSOR_UP_ONE: &str = "\x1b[1A";
const CLEAR_LINE: &str = "\x1b[2K\r";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub struct Ui {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    model: Arc<Mutex<Model>>,
}

impl Ui {
    /// Start the render thread over `model`.
    pub fn spawn(model: Arc<Mutex<Model>>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = std::thread::spawn({
            let stop = Arc::clone(&stop);
            let model = Arc::clone(&model);
            move || render_loop(&model, &stop)
        });
        Ui {
            stop,
            handle,
            model,
        }
    }

    /// Stop rendering, clear the status area, and print the final summary
    /// plus any warnings and errors nix reported.
    pub fn finish(self, build_succeeded: bool) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
        let model = self.model.lock().expect("model lock poisoned");
        let mut err = std::io::stderr().lock();
        for warning in &model.warnings {
            let _ = writeln!(err, "{DIM}{warning}{RESET}");
        }
        for error in &model.errors {
            let _ = writeln!(err, "{error}");
        }
        let elapsed = format_secs(model.started.elapsed().as_secs_f64());
        let verdict = if build_succeeded {
            "finished"
        } else {
            "FAILED"
        };
        let _ = writeln!(
            err,
            "{BOLD}build {verdict}{RESET} in {elapsed} ({} builds)",
            model.builds_done
        );
    }
}

fn render_loop(model: &Mutex<Model>, stop: &AtomicBool) {
    let mut prev_lines = 0usize;
    let mut tick = 0usize;
    while !stop.load(Ordering::Relaxed) {
        let frame = {
            let model = model.lock().expect("model lock poisoned");
            render(&model, terminal_width(), tick)
        };
        draw(&frame, &mut prev_lines);
        tick = tick.wrapping_add(1);
        std::thread::sleep(FRAME_INTERVAL);
    }
    // Erase the status area so finish() writes on a clean screen.
    draw(&[], &mut prev_lines);
}

/// Redraw in place: move to the top of the previous frame, rewrite each
/// line (clearing leftovers), and clear any excess lines from last frame.
fn draw(lines: &[String], prev_lines: &mut usize) {
    let mut out = String::new();
    for _ in 0..*prev_lines {
        out.push_str(CURSOR_UP_ONE);
    }
    for line in lines {
        out.push_str(CLEAR_LINE);
        out.push_str(line);
        out.push('\n');
    }
    for _ in lines.len()..*prev_lines {
        out.push_str(CLEAR_LINE);
        out.push('\n');
    }
    for _ in lines.len()..*prev_lines {
        out.push_str(CURSOR_UP_ONE);
    }
    *prev_lines = lines.len();
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(out.as_bytes());
    let _ = err.flush();
}

fn terminal_width() -> usize {
    terminal_size::terminal_size().map_or(100, |(w, _)| w.0 as usize)
}

fn render(model: &Model, width: usize, tick: usize) -> Vec<String> {
    let spinner = SPINNER[tick % SPINNER.len()];
    let elapsed = format_secs(model.started.elapsed().as_secs_f64());
    let mut lines = vec![truncate(
        &format!(
            "{BOLD}{spinner} nix-deploy:{RESET} {}/{} built, {} running, {} fetching, {elapsed}",
            model.builds_done,
            model.builds_expected.max(model.builds_done),
            model.running.len(),
            model.transfers.len(),
        ),
        width,
    )];

    let mut builds: Vec<&crate::log_model::Build> = model.running.values().collect();
    builds.sort_by_key(|b| b.started);
    for build in builds.iter().take(MAX_BUILD_ROWS) {
        let phase = build.phase.as_deref().unwrap_or("");
        lines.push(truncate(
            &format!(
                "  > {:<40} {:>7} {phase}",
                build.name,
                format_secs(build.started.elapsed().as_secs_f64())
            ),
            width,
        ));
        if let Some(log) = build.log_tail.back() {
            lines.push(truncate(&format!("      {DIM}{log}{RESET}"), width));
        }
    }
    if builds.len() > MAX_BUILD_ROWS {
        lines.push(format!(
            "  ... and {} more builds",
            builds.len() - MAX_BUILD_ROWS
        ));
    }

    for transfer in model.transfers.values().take(4) {
        lines.push(truncate(
            &format!(
                "  v {:<40} {} / {}",
                transfer.label,
                format_bytes(transfer.done_bytes),
                format_bytes(transfer.expected_bytes)
            ),
            width,
        ));
    }

    if !model.recent.is_empty() {
        let recent: Vec<String> = model
            .recent
            .iter()
            .map(|f| format!("{} ({})", f.name, format_secs(f.secs)))
            .collect();
        lines.push(truncate(
            &format!("  {DIM}recent: {}{RESET}", recent.join(", ")),
            width,
        ));
    }
    lines
}

/// Truncate to `width` terminal columns, counting chars (close enough for
/// store names) and ignoring the ANSI escapes we emit.
fn truncate(line: &str, width: usize) -> String {
    let visible = line.chars().filter(|c| *c != '\u{1b}').count();
    if visible <= width || width < 4 {
        return line.to_string();
    }
    // Escape-free prefix cut is fine: our styled lines only get cut in the
    // plain middle section, and RESET is re-appended.
    let cut: String = line.chars().take(width.saturating_sub(3)).collect();
    format!("{cut}...{RESET}")
}

fn format_secs(secs: f64) -> String {
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        let mins = (secs / 60.0).floor() as u64;
        format!("{mins}m{:02.0}s", secs % 60.0)
    }
}

fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let mib = bytes as f64 / MIB;
    if mib >= 1024.0 {
        format!("{:.1} GiB", mib / 1024.0)
    } else {
        format!("{mib:.1} MiB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(format_secs(3.25), "3.2s");
        assert_eq!(format_secs(62.0), "1m02s");
        assert_eq!(format_secs(600.0), "10m00s");
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0.0 MiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn truncates_wide_lines() {
        let line = "x".repeat(50);
        let cut = truncate(&line, 20);
        assert!(cut.starts_with(&"x".repeat(17)));
        assert!(cut.contains("..."));
        assert_eq!(truncate("short", 20), "short");
    }
}
