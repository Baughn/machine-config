//! `ratatui` rendering for the top system-monitor bar, responsive two-pane
//! layout, build/deploy pane, interactive modals, and footer hints.

use std::sync::Mutex;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Sparkline, Wrap};
use ratatui::Frame;

use crate::log_model::Model;
use crate::sysmon::SysMon;
use crate::tui::state::{
    format_rev_span, AppState, FlakeModalState, LineStyle, MachineOutcome, Pane, Stage,
};

const SPINNER: &[char] = &['|', '/', '-', '\\'];
const SIDE_BY_SIDE_MIN_WIDTH: u16 = 100;

/// Draw one complete frame of the TUI.
pub fn draw(frame: &mut Frame<'_>, app: &AppState, sysmon: &SysMon, build_model: &Mutex<Model>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_top_bar(frame, chunks[0], sysmon);
    render_body(frame, chunks[1], app, build_model);
    render_footer(frame, chunks[2], app);
    render_modal_if_any(frame, frame.area(), app);
}

fn render_top_bar(frame: &mut Frame<'_>, area: Rect, sysmon: &SysMon) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let (cpu_title, mem_title) = match sysmon.latest() {
        Some(sample) => {
            let load = sample
                .load1
                .map(|l| format!("  load {l:.2}"))
                .unwrap_or_default();
            let cpu = format!(" CPU {:>4.1}%{load} ", sample.cpu_pct);
            let used = SysMon::format_gib(sample.mem_used_bytes);
            let total = SysMon::format_gib(sample.mem_total_bytes);
            let mem = match sample.arc_bytes {
                Some(arc) => {
                    let arc_str = SysMon::format_gib(arc);
                    format!(" MEM {used} (+{arc_str} ARC) / {total} ")
                }
                None => format!(" MEM {used} / {total} "),
            };
            (cpu, mem)
        }
        None => (" CPU --.-% ".to_string(), " MEM -- / -- ".to_string()),
    };

    let cpu_data = tail_slice(
        sysmon.cpu_history(),
        halves[0].width.saturating_sub(2) as usize,
    );
    let cpu_spark = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    cpu_title,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .data(&cpu_data)
        .max(100)
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(cpu_spark, halves[0]);

    let mem_data = tail_slice(
        sysmon.mem_history(),
        halves[1].width.saturating_sub(2) as usize,
    );
    let mem_spark = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    mem_title,
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .data(&mem_data)
        .max(100)
        .style(Style::default().fg(Color::Green));
    frame.render_widget(mem_spark, halves[1]);
}

fn tail_slice(history: &std::collections::VecDeque<u64>, width: usize) -> Vec<u64> {
    if width == 0 {
        return Vec::new();
    }
    let skip = history.len().saturating_sub(width);
    history.iter().skip(skip).copied().collect()
}

fn render_body(frame: &mut Frame<'_>, area: Rect, app: &AppState, build_model: &Mutex<Model>) {
    if area.width >= SIDE_BY_SIDE_MIN_WIDTH {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        render_left_pane(frame, panes[0], app, false);
        render_right_pane(frame, panes[1], app, build_model, false);
    } else {
        match app.active_pane {
            Pane::Left => render_left_pane(frame, area, app, true),
            Pane::Right => render_right_pane(frame, area, app, build_model, true),
        }
    }
}

fn pane_border_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_left_pane(frame: &mut Frame<'_>, area: Rect, app: &AppState, narrow: bool) {
    let active = app.active_pane == Pane::Left;
    let total = app.reviewable_inputs.len();
    let fetched = app.fetched_diff_count();
    let progress = if total > 0 && fetched < total {
        format!(" [fetching {fetched}/{total}]")
    } else if total > 0 {
        format!(" [{total} inputs]")
    } else {
        String::new()
    };
    let narrow_hint = if narrow {
        " (Tab -> Build & Deploy)"
    } else {
        ""
    };
    let title = format!(" Diffs & Commits{progress}{narrow_hint} ");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(pane_border_style(active));

    let inner = block.inner(area);
    let visible_height = inner.height as usize;

    let lines: Vec<Line<'_>> = app
        .left_lines
        .iter()
        .skip(app.left_scroll)
        .take(visible_height.max(1))
        .map(|sl| Line::from(Span::styled(sl.text.as_str(), style_for_line(sl.style))))
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn style_for_line(style: LineStyle) -> Style {
    match style {
        LineStyle::SectionHeader => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        LineStyle::InputHeader => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        LineStyle::FileHeader => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        LineStyle::HunkHeader => Style::default().fg(Color::Cyan),
        LineStyle::Added => Style::default().fg(Color::Green),
        LineStyle::Removed => Style::default().fg(Color::Red),
        LineStyle::Context => Style::default(),
        LineStyle::Commit => Style::default().fg(Color::White),
        LineStyle::Badge => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        LineStyle::Dim => Style::default().fg(Color::DarkGray),
        LineStyle::Warning => Style::default().fg(Color::Yellow),
    }
}

fn render_right_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &AppState,
    build_model: &Mutex<Model>,
    narrow: bool,
) {
    let active = app.active_pane == Pane::Right;
    let narrow_hint = if narrow {
        " (Tab -> Diffs & Commits)"
    } else {
        ""
    };
    let title = format!(" Build & Deploy{narrow_hint} ");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(pane_border_style(active));

    let inner = block.inner(area);
    let content = build_right_pane_lines(app, build_model);

    let max_scroll = content.len().saturating_sub(inner.height as usize);
    app.right_max_scroll.set(max_scroll);
    let scroll = if app.right_follow_tail {
        max_scroll
    } else {
        app.right_scroll.min(max_scroll)
    };

    let visible: Vec<Line<'_>> = content
        .into_iter()
        .skip(scroll)
        .take(inner.height.max(1) as usize)
        .collect();

    let paragraph = Paragraph::new(visible)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn build_right_pane_lines(app: &AppState, build_model: &Mutex<Model>) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let spinner = SPINNER[app.tick % SPINNER.len()];

    // Machine overview header once plans exist.
    if !app.machine_plans.is_empty() {
        let mut spans = vec![Span::styled(
            "Machines: ",
            Style::default().add_modifier(Modifier::BOLD),
        )];
        for (i, plan) in app.machine_plans.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  |  "));
            }
            let badge_color = match &plan.outcome {
                MachineOutcome::Activated(_) | MachineOutcome::UpToDate => Color::Green,
                MachineOutcome::RebootCancelledAfterBoot => Color::Yellow,
                MachineOutcome::Failed(_) => Color::Red,
                MachineOutcome::Pending => {
                    if app.active_machine_idx == Some(i) {
                        Color::Cyan
                    } else {
                        Color::DarkGray
                    }
                }
            };
            spans.push(Span::styled(
                format!("{} [{}]", plan.machine.name, plan.outcome.badge()),
                Style::default()
                    .fg(badge_color)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::from(""));
    }

    // Recent status events.
    if !app.status_log.is_empty() {
        let tail_start = app.status_log.len().saturating_sub(8);
        for entry in &app.status_log[tail_start..] {
            let style = if entry.starts_with("warning:") {
                Style::default().fg(Color::Yellow)
            } else if entry.starts_with("error:") {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(Span::styled(entry.clone(), style)));
        }
        lines.push(Line::from(""));
    }

    match &app.stage {
        Stage::InspectingRepo => {
            lines.push(Line::from(format!(
                "{spinner} Inspecting repository state ..."
            )));
        }
        Stage::RepoSyncPrompt { .. } => {
            lines.push(Line::from("Waiting for repo sync decision ..."));
        }
        Stage::Rebasing => {
            lines.push(Line::from(format!("{spinner} Rebasing onto trunk ...")));
        }
        Stage::CommitMessagePrompt { .. } => {
            lines.push(Line::from("Waiting for working copy commit message ..."));
        }
        Stage::LineageCheck => {
            lines.push(Line::from(format!(
                "{spinner} Checking running revision lineage on selected machines ..."
            )));
        }
        Stage::FlakeUpdateModal(_) => {
            lines.push(Line::from("Waiting for flake update decision ..."));
        }
        Stage::Building => {
            append_build_model_lines(&mut lines, build_model, spinner);
        }
        Stage::ComputingMachineDiffs => {
            lines.push(Line::from(Span::styled(
                format!(
                    "{spinner} Build succeeded; computing closure diffs and reboot verdicts ..."
                ),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        Stage::ReviewGate => {
            lines.push(Line::from(Span::styled(
                "Build done -- finish reviewing diffs on left, then [a]pprove & continue / [q] abort",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            if app.fetched_diff_count() < app.reviewable_inputs.len() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "{spinner} Still fetching flake input diffs ({}/{}) ...",
                        app.fetched_diff_count(),
                        app.reviewable_inputs.len()
                    ),
                    Style::default().fg(Color::Cyan),
                )));
            }
            lines.push(Line::from(""));
            for plan in &app.machine_plans {
                append_machine_summary_lines(&mut lines, plan, false);
                lines.push(Line::from(""));
            }
        }
        Stage::MachinePrompt { machine_idx } => {
            if let Some(plan) = app.machine_plans.get(*machine_idx) {
                append_machine_summary_lines(&mut lines, plan, true);
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(
                        "{}: [s]witch  [b]oot only  [r] boot + reboot  [q] abort",
                        plan.machine.name
                    ),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
        }
        Stage::Activating {
            machine_idx,
            action,
        } => {
            if let Some(plan) = app.machine_plans.get(*machine_idx) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "{spinner} Deploying {} ({}) ...",
                        plan.machine.name,
                        action.label()
                    ),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                for log_line in &plan.activation_logs {
                    let style = if log_line.starts_with("warning:") {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    };
                    lines.push(Line::from(Span::styled(log_line.clone(), style)));
                }
            }
        }
        Stage::PushBeforeLocalReboot { machine_idx, .. } => {
            if let Some(plan) = app.machine_plans.get(*machine_idx) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "{}: boot entry set; waiting for push-before-reboot choice ...",
                        plan.machine.name
                    ),
                    Style::default().fg(Color::Yellow),
                )));
                for log_line in &plan.activation_logs {
                    lines.push(Line::from(log_line.clone()));
                }
            }
        }
        Stage::PushAfterDeploy { .. } | Stage::ConfirmAbortRollback { .. } => {
            for plan in &app.machine_plans {
                lines.push(Line::from(format!(
                    "{}: {}",
                    plan.machine.name,
                    plan.outcome.badge()
                )));
            }
        }
    }

    lines
}

fn append_build_model_lines(
    lines: &mut Vec<Line<'static>>,
    build_model: &Mutex<Model>,
    spinner: char,
) {
    let model = build_model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let elapsed = format_secs(model.started.elapsed().as_secs_f64());
    let expected = model.builds_expected.max(model.builds_done);
    lines.push(Line::from(Span::styled(
        format!(
            "{spinner} nix build: {}/{} built, {} running, {} fetching, {elapsed}",
            model.builds_done,
            expected,
            model.running.len(),
            model.transfers.len(),
        ),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    if model.running.is_empty() && model.transfers.is_empty() && model.builds_done == 0 {
        lines.push(Line::from(Span::styled(
            "  evaluating flake ...",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let mut builds: Vec<&crate::log_model::Build> = model.running.values().collect();
    builds.sort_by_key(|b| b.started);
    const MAX_BUILD_ROWS: usize = 14;
    for build in builds.iter().take(MAX_BUILD_ROWS) {
        let phase = build.phase.as_deref().unwrap_or("");
        let dur = format_secs(build.started.elapsed().as_secs_f64());
        lines.push(Line::from(vec![
            Span::styled("  > ", Style::default().fg(Color::Green)),
            Span::styled(
                format!("{:<36}", build.name),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {:>7} ", dur), Style::default().fg(Color::Yellow)),
            Span::styled(phase.to_string(), Style::default().fg(Color::Cyan)),
        ]));
        if let Some(log) = build.log_tail.back() {
            lines.push(Line::from(Span::styled(
                format!("      {log}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    if builds.len() > MAX_BUILD_ROWS {
        lines.push(Line::from(Span::styled(
            format!("  ... and {} more builds", builds.len() - MAX_BUILD_ROWS),
            Style::default().fg(Color::DarkGray),
        )));
    }

    for transfer in model.transfers.values().take(6) {
        lines.push(Line::from(vec![
            Span::styled("  v ", Style::default().fg(Color::Blue)),
            Span::raw(format!(
                "{:<36} {} / {}",
                transfer.label,
                format_bytes(transfer.done_bytes),
                format_bytes(transfer.expected_bytes),
            )),
        ]));
    }

    if !model.recent.is_empty() {
        let recent: Vec<String> = model
            .recent
            .iter()
            .map(|f| format!("{} ({})", f.name, format_secs(f.secs)))
            .collect();
        lines.push(Line::from(Span::styled(
            format!("  recent: {}", recent.join(", ")),
            Style::default().fg(Color::DarkGray),
        )));
    }

    for warning in model.warnings.iter().rev().take(4).rev() {
        lines.push(Line::from(Span::styled(
            format!("  warn: {warning}"),
            Style::default().fg(Color::Yellow),
        )));
    }
    for error in model.errors.iter().rev().take(6).rev() {
        lines.push(Line::from(Span::styled(
            format!("  err: {error}"),
            Style::default().fg(Color::Red),
        )));
    }
}

fn append_machine_summary_lines(
    lines: &mut Vec<Line<'static>>,
    plan: &crate::tui::state::MachinePlan,
    include_full_nvd: bool,
) {
    lines.push(Line::from(Span::styled(
        format!("=== {} ({}) ===", plan.machine.name, plan.machine.target),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    if plan.up_to_date {
        lines.push(Line::from(Span::styled(
            "  already up to date",
            Style::default().fg(Color::Green),
        )));
        return;
    }

    if plan.verdict.reboot_recommended() {
        lines.push(Line::from(Span::styled(
            "  REBOOT RECOMMENDED:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for reason in &plan.verdict.reasons {
            lines.push(Line::from(Span::styled(
                format!("    - {reason}"),
                Style::default().fg(Color::Yellow),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  no reboot-worthy changes",
            Style::default().fg(Color::Green),
        )));
    }

    let limit = if include_full_nvd {
        plan.nvd_lines.len()
    } else {
        plan.nvd_lines.len().min(12)
    };
    if !plan.nvd_lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Package diff (nvd):",
            Style::default().fg(Color::DarkGray),
        )));
        for line in &plan.nvd_lines[..limit] {
            let style = if line.starts_with("[A") || line.contains(" + ") {
                Style::default().fg(Color::Green)
            } else if line.starts_with("[R") || line.contains(" - ") {
                Style::default().fg(Color::Red)
            } else if line.starts_with("[U") || line.starts_with("[D") || line.starts_with("[C") {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(format!("    {line}"), style)));
        }
        if plan.nvd_lines.len() > limit {
            lines.push(Line::from(Span::styled(
                format!("    ... ({} more nvd lines)", plan.nvd_lines.len() - limit),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let hints = match &app.stage {
        Stage::RepoSyncPrompt { .. } => "[r]ebase onto trunk  [c]ontinue without rebasing  [q]uit",
        Stage::CommitMessagePrompt { .. } => "[Enter] commit & continue  [Esc] quit",
        Stage::FlakeUpdateModal(FlakeModalState::Running {
            want_apply_when_ready: false,
        }) => "[y] apply when ready  [n] skip update  [q] quit",
        Stage::FlakeUpdateModal(FlakeModalState::Running {
            want_apply_when_ready: true,
        }) => "Waiting for flake update...  [n] cancel & skip  [q] quit",
        Stage::FlakeUpdateModal(FlakeModalState::Ready { .. }) => {
            "[y] apply & build  [n] skip & build  [q] quit"
        }
        Stage::FlakeUpdateModal(FlakeModalState::Failed { .. }) => {
            "[Enter/n] continue without update  [q] quit"
        }
        Stage::ReviewGate => {
            "[a]pprove & continue  [Tab] pane  [j/k] scroll  [n/N] next/prev  [f] filter  [e] expand  [q] abort"
        }
        Stage::MachinePrompt { .. } => {
            "[s]witch  [b]oot  [r] boot+reboot  [Tab] pane  [j/k] scroll  [f] filter  [e] expand  [q] abort"
        }
        Stage::Activating { .. } => {
            "[Tab] pane  [j/k] scroll  [PgUp/PgDn] page  [n/N] next/prev  [f] filter  [e] expand  (Ctrl-C force exit)"
        }
        Stage::PushBeforeLocalReboot { .. } => {
            "[p]ush to master & reboot  [r]eboot without pushing  [c]ancel reboot"
        }
        Stage::PushAfterDeploy { .. } => "[p]ush to master  [Enter/q] exit",
        Stage::ConfirmAbortRollback { .. } => {
            "[r]evert Bumps (jj op restore)  [k]eep Bumps"
        }
        _ => "[Tab] pane  [j/k] scroll  [PgUp/PgDn] page  [n/N] next/prev  [f] filter  [e] expand  [q] quit",
    };

    let bar = Paragraph::new(Line::from(Span::styled(
        format!(" {hints}"),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(bar, area);
}

fn render_modal_if_any(frame: &mut Frame<'_>, full: Rect, app: &AppState) {
    let spinner = SPINNER[app.tick % SPINNER.len()];
    match &app.stage {
        Stage::RepoSyncPrompt { incoming } => {
            let mut lines = vec![
                Line::from(Span::styled(
                    format!(
                        "trunk() has {} incoming commit(s) not yet in @:",
                        incoming.len()
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            for c in incoming.iter().take(12) {
                lines.push(Line::from(format!("  {}  {}", c.short_id, c.description)));
            }
            if incoming.len() > 12 {
                lines.push(Line::from(format!(
                    "  ... and {} more",
                    incoming.len() - 12
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[r]ebase onto trunk   [c]ontinue without rebasing   [q]uit",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            render_centered_modal(frame, full, " Repository Sync ", lines, Color::Yellow);
        }
        Stage::CommitMessagePrompt {
            input,
            cursor,
            changed_files,
            validation_error,
        } => {
            let mut lines = vec![
                Line::from("Working copy (@) has unnamed changes. Enter a commit message:"),
                Line::from(""),
            ];
            for file in changed_files.iter().take(8) {
                lines.push(Line::from(Span::styled(
                    format!("  M {file}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            if changed_files.len() > 8 {
                lines.push(Line::from(Span::styled(
                    format!("  ... and {} more file(s)", changed_files.len() - 8),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(""));

            let clamped = (*cursor).min(input.len());
            let before = &input[..clamped];
            let after = &input[clamped..];
            lines.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Cyan)),
                Span::raw(before.to_string()),
                Span::styled("_", Style::default().fg(Color::Yellow)),
                Span::raw(after.to_string()),
            ]));
            if let Some(err) = validation_error {
                lines.push(Line::from(Span::styled(
                    err.clone(),
                    Style::default().fg(Color::Red),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[Enter] describe & new   [Esc] quit",
                Style::default().fg(Color::Cyan),
            )));
            render_centered_modal(frame, full, " Commit Working Copy ", lines, Color::Cyan);
        }
        Stage::FlakeUpdateModal(modal) => {
            let (title, border, lines) = match modal {
                FlakeModalState::Running {
                    want_apply_when_ready,
                } => {
                    let msg = if *want_apply_when_ready {
                        format!("{spinner} Waiting for nix flake update to finish ...")
                    } else {
                        format!("{spinner} Checking for flake updates in background ...")
                    };
                    let hint = if *want_apply_when_ready {
                        "Will apply and build automatically when ready.  [n] cancel & skip  [q] quit"
                    } else {
                        "[y] Apply when ready   [n] Skip update   [q] Quit"
                    };
                    (
                        " Flake Update ",
                        Color::Cyan,
                        vec![
                            Line::from(msg),
                            Line::from(""),
                            Line::from(Span::styled(
                                "Updating candidate lockfile in a temporary directory; ./flake.lock is untouched.",
                                Style::default().fg(Color::DarkGray),
                            )),
                            Line::from(""),
                            Line::from(Span::styled(
                                hint,
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            )),
                        ],
                    )
                }
                FlakeModalState::Ready { result } => {
                    let review_count = result.reviewable_changes().len();
                    let mut lines = vec![
                        Line::from(Span::styled(
                            format!(
                                "{} flake input(s) updated ({review_count} will be diffed for review):",
                                result.changes.len()
                            ),
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                        Line::from(""),
                    ];
                    for change in result.changes.iter().take(14) {
                        let (tag, color) = if change.is_ignored {
                            ("[core]  ", Color::DarkGray)
                        } else {
                            ("[review]", Color::Yellow)
                        };
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {tag} "), Style::default().fg(color)),
                            Span::raw(format!(
                                "{} ({}) {}",
                                change.names.join(", "),
                                change.repo_id,
                                format_rev_span(change)
                            )),
                        ]));
                    }
                    if result.changes.len() > 14 {
                        lines.push(Line::from(format!(
                            "  ... and {} more input(s)",
                            result.changes.len() - 14
                        )));
                    }
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "[y] Apply & build   [n] Skip & build   [q] Quit",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                    (" Flake Updates Available ", Color::Yellow, lines)
                }
                FlakeModalState::Failed { error } => (
                    " Flake Update Failed ",
                    Color::Red,
                    vec![
                        Line::from(Span::styled(error.clone(), Style::default().fg(Color::Red))),
                        Line::from(""),
                        Line::from(Span::styled(
                            "[Enter/n] Continue without update   [q] Quit",
                            Style::default().fg(Color::Cyan),
                        )),
                    ],
                ),
            };
            render_centered_modal(frame, full, title, lines, border);
        }
        Stage::ConfirmAbortRollback {
            reason,
            op_id,
            is_error,
        } => {
            let short_op = if op_id.len() > 12 {
                &op_id[..12]
            } else {
                op_id
            };
            let title = if *is_error {
                " Failed After Bumps Commit "
            } else {
                " Abort After Bumps Commit "
            };
            let lines = vec![
                Line::from(Span::styled(
                    reason.clone(),
                    Style::default().fg(if *is_error { Color::Red } else { Color::Yellow }),
                )),
                Line::from(""),
                Line::from(format!(
                    "A \"Bumps\" commit was recorded for flake.lock (pre-bump op {short_op})."
                )),
                Line::from("Revert it with `jj op restore` or keep it in history?"),
                Line::from(""),
                Line::from(Span::styled(
                    "[r]evert Bumps (jj op restore)   [k]eep Bumps",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
            ];
            render_centered_modal(frame, full, title, lines, Color::Red);
        }
        Stage::PushBeforeLocalReboot {
            unpushed,
            pushing,
            push_error,
            ..
        } => {
            let mut lines = vec![
                Line::from(Span::styled(
                    format!(
                        "Local boot entry is set. {} unpushed commit(s) before rebooting:",
                        unpushed.len()
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            for c in unpushed.iter().take(10) {
                lines.push(Line::from(format!("  {}  {}", c.short_id, c.description)));
            }
            if let Some(err) = push_error {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Push failed: {err}"),
                    Style::default().fg(Color::Red),
                )));
            }
            lines.push(Line::from(""));
            if *pushing {
                lines.push(Line::from(format!(
                    "{spinner} Pushing master to origin ..."
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "[p]ush to master & reboot   [r]eboot without pushing   [c]ancel",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            render_centered_modal(frame, full, " Push Before Reboot ", lines, Color::Yellow);
        }
        Stage::PushAfterDeploy {
            unpushed,
            pushing,
            push_error,
        } => {
            let mut lines = vec![
                Line::from(Span::styled(
                    format!(
                        "All machines deployed. {} unpushed commit(s) ready to push:",
                        unpushed.len()
                    ),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            for c in unpushed.iter().take(10) {
                lines.push(Line::from(format!("  {}  {}", c.short_id, c.description)));
            }
            if let Some(err) = push_error {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Push failed: {err}"),
                    Style::default().fg(Color::Red),
                )));
            }
            lines.push(Line::from(""));
            if *pushing {
                lines.push(Line::from(format!(
                    "{spinner} Pushing master to origin ..."
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "[p]ush to master   [Enter/q] exit",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            render_centered_modal(frame, full, " Deploy Complete ", lines, Color::Green);
        }
        _ => {}
    }
}

fn render_centered_modal(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    border_color: Color,
) {
    let width = area.width.saturating_sub(6).clamp(40.min(area.width), 88);
    let desired_h = (lines.len() as u16).saturating_add(4);
    let height = desired_h
        .min(area.height.saturating_sub(2))
        .max(6.min(area.height));

    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            title,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(border_color));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);
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
