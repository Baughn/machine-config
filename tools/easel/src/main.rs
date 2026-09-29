//! easel: a raster painting tool driven by a replayable DSL, designed for a
//! painter that works through a shell and looks at PNGs.

mod color;
mod doc;
mod dsl;
mod marks;
mod ops;
mod raster;
mod render;
mod shape;
mod view;

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::doc::{Document, Reach};
use crate::render::{blend_name, LayerFilter, RenderState, Viewport};
use crate::view::{Crop, Grid};

#[derive(Parser)]
#[command(
    name = "easel",
    version,
    about = "Raster painting via a replayable DSL of marks and layers",
    after_help = "Start with `easel ops workflow`, then `easel ops` for the full DSL reference."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new document directory.
    New {
        doc: PathBuf,
        /// Canvas size, e.g. 768x1024.
        #[arg(long, value_parser = parse_size)]
        size: (u32, u32),
        /// Background colour (any DSL colour).
        #[arg(long, default_value = "white")]
        bg: String,
    },
    /// Apply a script as one commit (FILE omitted or "-" reads stdin).
    Apply {
        doc: PathBuf,
        file: Option<PathBuf>,
        /// Commit message (default: the script's first comment line).
        #[arg(short, long)]
        message: Option<String>,
        /// Run and report, but do not commit.
        #[arg(long)]
        dry_run: bool,
        /// Report only mark ops that placed nothing or not every mark, and
        /// the final line.
        #[arg(short, long)]
        quiet: bool,
    },
    /// Replace commit N's script (FILE omitted or "-" reads stdin) and
    /// replay the commits after it.
    Amend {
        doc: PathBuf,
        n: usize,
        file: Option<PathBuf>,
        /// New commit message (default: keep the old one).
        #[arg(short, long)]
        message: Option<String>,
        /// As for apply.
        #[arg(short, long)]
        quiet: bool,
    },
    /// Undo the last N commits (default 1).
    Undo {
        doc: PathBuf,
        #[arg(default_value_t = 1)]
        n: usize,
    },
    /// Redo N undone commits (default 1).
    Redo {
        doc: PathBuf,
        #[arg(default_value_t = 1)]
        n: usize,
    },
    /// List commits: index, message, op count, line count.
    Log { doc: PathBuf },
    /// Print the script of commit N.
    Show { doc: PathBuf, n: usize },
    /// Canvas size, commits, variables and layers.
    Info { doc: PathBuf },
    /// Render a PNG to look at (whole canvas or a zoomed crop).
    View {
        doc: PathBuf,
        /// Output file (default DOC/view.png).
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Canvas region X,Y,W,H (may start off-canvas, e.g. -10,-10,50,50).
        #[arg(long, value_parser = parse_crop, allow_hyphen_values = true)]
        crop: Option<[f32; 4]>,
        /// Long side of the output in px.
        #[arg(long, default_value_t = 1000)]
        max: u32,
        /// Grid spacing in canvas px, `auto` or `off`
        /// (default: auto for crops, off otherwise).
        #[arg(long, value_parser = parse_grid)]
        grid: Option<Grid>,
        /// Show only this layer (hidden or not) over a checkerboard.
        #[arg(long)]
        solo: Option<String>,
        /// Hide these layers.
        #[arg(long, value_delimiter = ',')]
        hide: Vec<String>,
        /// Show these layers even if hidden.
        #[arg(long, value_delimiter = ',')]
        show: Vec<String>,
        /// Render the state after commit N (0 = blank canvas).
        #[arg(long)]
        at: Option<usize>,
        /// Show alpha as greyscale.
        #[arg(long)]
        alpha: bool,
        /// Show the actual canvas pixels, enlarged by a whole number
        /// (nearest neighbour), instead of re-rendering zoomed crops.
        #[arg(long)]
        pixels: bool,
    },
    /// Print colours at canvas points.
    Sample {
        doc: PathBuf,
        /// Points X,Y.
        #[arg(required = true, value_parser = parse_xy)]
        points: Vec<(f32, f32)>,
        /// Sample this layer instead of the composite.
        #[arg(long)]
        layer: Option<String>,
        /// Average over a disc of this radius (canvas px).
        #[arg(long, default_value_t = 0.0)]
        radius: f32,
    },
    /// Render the final image, optionally at a higher resolution.
    Export {
        doc: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Re-render every mark at this scale.
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
        #[arg(long, value_delimiter = ',')]
        hide: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        show: Vec<String>,
    },
    /// Copy a PNG into the document's assets (for the `image` op).
    Import {
        doc: PathBuf,
        file: PathBuf,
        /// Asset name (default: the file name without extension).
        #[arg(long)]
        name: Option<String>,
    },
    /// DSL reference: all ops, or one op/topic in detail.
    Ops { op: Option<String> },
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("expected WxH, got '{s}'"))?;
    let n = |v: &str| v.parse::<u32>().map_err(|_| format!("bad size '{s}'"));
    Ok((n(w)?, n(h)?))
}

fn parse_crop(s: &str) -> Result<[f32; 4], String> {
    let v: Vec<f32> = s
        .split(',')
        .map(dsl::parse_number)
        .collect::<Result<_, _>>()?;
    match v.as_slice() {
        [x, y, w, h] if *w > 0.0 && *h > 0.0 => Ok([*x, *y, *w, *h]),
        _ => Err(format!("expected X,Y,W,H with positive W and H, got '{s}'")),
    }
}

fn parse_grid(s: &str) -> Result<Grid, String> {
    match s {
        "off" => Ok(Grid::Off),
        "auto" => Ok(Grid::Auto),
        n => match dsl::parse_number(n) {
            Ok(v) if v >= 1.0 => Ok(Grid::Every(v)),
            _ => Err(format!(
                "expected a spacing >= 1, 'auto' or 'off', got '{s}'"
            )),
        },
    }
}

fn parse_xy(s: &str) -> Result<(f32, f32), String> {
    dsl::parse_point(s).map(|p| (p.x, p.y))
}

/// clap reads `-5,3` as a flag. For `sample`, move every X,Y point to the
/// end behind `--` so negative coordinates work without the user having to
/// know that. Order among the points is kept; an explicit `--` disables this.
fn normalize_args(args: Vec<OsString>) -> Vec<OsString> {
    let is_negative_point = |a: &OsString| {
        a.to_str()
            .is_some_and(|s| s.starts_with('-') && dsl::parse_point(s).is_ok())
    };
    if args.get(1).and_then(|a| a.to_str()) != Some("sample")
        || args.iter().any(|a| a == "--")
        || !args.iter().any(is_negative_point)
    {
        return args;
    }
    let is_point = |a: &OsString| a.to_str().is_some_and(|s| dsl::parse_point(s).is_ok());
    let mut rest = Vec::with_capacity(args.len() + 1);
    let mut points = Vec::new();
    for (i, a) in args.into_iter().enumerate() {
        if i > 1 && is_point(&a) {
            points.push(a);
        } else {
            rest.push(a);
        }
    }
    rest.push("--".into());
    rest.extend(points);
    rest
}

fn main() -> ExitCode {
    let cli = Cli::parse_from(normalize_args(std::env::args_os().collect()));
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<()> {
    match command {
        Command::New { doc, size, bg } => {
            Document::create(&doc, size.0, size.1, &bg)?;
            println!(
                "created {} ({}x{}, background {bg})",
                doc.display(),
                size.0,
                size.1
            );
            Ok(())
        }
        Command::Apply {
            doc,
            file,
            message,
            dry_run,
            quiet,
        } => cmd_apply(&doc, file.as_deref(), message, dry_run, quiet),
        Command::Amend {
            doc,
            n,
            file,
            message,
            quiet,
        } => {
            let script = read_script(file.as_deref())?;
            let mut d = Document::load(&doc)?;
            let discarded = d.file.redo.len();
            let started = Instant::now();
            let report = d.amend(n, &script, message.as_deref())?;
            print_report(&report, quiet);
            println!(
                "ok: amended commit {n} \"{}\" and replayed {} later commits, {:.2}s",
                d.file.commits[n - 1].message,
                d.file.commits.len() - n,
                started.elapsed().as_secs_f32()
            );
            print_discarded(discarded);
            Ok(())
        }
        Command::Undo { doc, n } => {
            let mut d = Document::load(&doc)?;
            d.undo(n)?;
            println!(
                "undid {n}; now at commit {} ({} redoable; the next apply or amend discards them)",
                d.file.commits.len(),
                d.file.redo.len()
            );
            Ok(())
        }
        Command::Redo { doc, n } => {
            let mut d = Document::load(&doc)?;
            d.redo(n)?;
            println!(
                "redid {n}; now at commit {} ({} redoable)",
                d.file.commits.len(),
                d.file.redo.len()
            );
            Ok(())
        }
        Command::Log { doc } => cmd_log(&doc),
        Command::Show { doc, n } => {
            let d = Document::load(&doc)?;
            let commit = n
                .checked_sub(1)
                .and_then(|i| d.file.commits.get(i))
                .ok_or_else(|| anyhow!("no commit {n} (1..={})", d.file.commits.len()))?;
            // The header goes to stderr so that `show N > file` holds the
            // script exactly: an extra line would shift every line number,
            // and with them the seeds, when the file is amended back.
            eprintln!("# commit {n}: {}", commit.message);
            print!("{}", commit.script);
            if !commit.script.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        Command::Info { doc } => cmd_info(&doc),
        Command::View {
            doc,
            out,
            crop,
            max,
            grid,
            solo,
            hide,
            show,
            at,
            alpha,
            pixels,
        } => {
            let opts = ViewOpts {
                crop,
                max,
                grid,
                filter: LayerFilter { solo, hide, show },
                at,
                alpha,
                pixels,
            };
            cmd_view(&doc, out, &opts)
        }
        Command::Sample {
            doc,
            points,
            layer,
            radius,
        } => cmd_sample(&doc, &points, layer.as_deref(), radius),
        Command::Export {
            doc,
            out,
            scale,
            hide,
            show,
        } => {
            if !(0.05..=16.0).contains(&scale) {
                bail!("--scale must be within 0.05..16");
            }
            let d = Document::load(&doc)?;
            let started = Instant::now();
            let state = d.render_at(d.file.commits.len(), scale)?;
            let filter = LayerFilter {
                solo: None,
                hide,
                show,
            };
            let img = state.composite(&filter).map_err(|e| anyhow!(e))?;
            img.save_png(&out)
                .with_context(|| format!("writing {}", out.display()))?;
            println!(
                "wrote {} ({}x{}, scale {scale}, {:.2}s)",
                out.display(),
                img.width(),
                img.height(),
                started.elapsed().as_secs_f32()
            );
            Ok(())
        }
        Command::Import { doc, file, name } => cmd_import(&doc, &file, name),
        Command::Ops { op } => {
            let text = ops::reference(op.as_deref()).map_err(|e| anyhow!(e))?;
            print!("{text}");
            Ok(())
        }
    }
}

/// Default commit message: the script's first non-empty comment line, else
/// a summary of its ops such as "fill, stipple x3".
fn default_message(script: &str) -> String {
    let comment = script
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix('#'))
        .map(|m| m.trim_start_matches('#').trim())
        .find(|m| !m.is_empty());
    if let Some(c) = comment {
        return c.to_string();
    }
    // A script that fails to parse is rejected by apply anyway.
    let Ok(stmts) = dsl::parse_script(script) else {
        return String::new();
    };
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for s in &stmts {
        match counts.iter_mut().find(|(op, _)| *op == s.op) {
            Some((_, n)) => *n += 1,
            None => counts.push((&s.op, 1)),
        }
    }
    counts
        .iter()
        .map(|(op, n)| {
            if *n > 1 {
                format!("{op} x{n}")
            } else {
                op.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A script from FILE, or stdin when FILE is omitted or "-".
fn read_script(file: Option<&Path>) -> Result<String> {
    match file {
        None => read_stdin(),
        Some(p) if p == Path::new("-") => read_stdin(),
        Some(p) => fs::read_to_string(p).with_context(|| format!("reading {}", p.display())),
    }
}

/// Print apply's per-op report; `quiet` keeps only mark ops that placed
/// nothing or could not place every candidate.
fn print_report(report: &[String], quiet: bool) {
    for line in report.iter().filter(|l| !quiet || notable_report(l)) {
        println!("{line}");
    }
}

/// Whether a report line (`LINE OP REPORT`) is a mark op that placed
/// nothing or not every candidate. Only the report part is searched, so a
/// layer or variable named e.g. `outside` does not count.
fn notable_report(line: &str) -> bool {
    let op = line.split_whitespace().nth(1).unwrap_or("");
    let report = line.split_once(" -> ").map_or("", |(_, r)| r);
    matches!(op, "dot" | "stipple" | "pointillize" | "dabs")
        && (report.starts_with("0 marks") || report.contains("not placed"))
}

fn cmd_apply(
    doc: &Path,
    file: Option<&Path>,
    message: Option<String>,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    let script = read_script(file)?;
    let mut d = Document::load(doc)?;
    let message = message.unwrap_or_else(|| default_message(&script));
    let discarded = d.file.redo.len();
    let started = Instant::now();
    let report = d.apply(&script, &message, dry_run)?;
    print_report(&report, quiet);
    let secs = started.elapsed().as_secs_f32();
    let ops = report.len();
    if dry_run {
        println!("ok (dry run, not committed): {ops} ops, render {secs:.2}s");
    } else {
        println!(
            "ok: commit {} \"{message}\": {ops} ops, render {secs:.2}s",
            d.file.commits.len()
        );
        print_discarded(discarded);
    }
    Ok(())
}

/// The note after an apply or amend that dropped undone commits.
fn print_discarded(discarded: usize) {
    if discarded > 0 {
        println!("note: discarded {discarded} undone commits; they can no longer be redone");
    }
}

fn read_stdin() -> Result<String> {
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .context("reading script from stdin")?;
    Ok(s)
}

fn cmd_log(doc: &Path) -> Result<()> {
    let d = Document::load(doc)?;
    if d.file.commits.is_empty() {
        println!("no commits");
    }
    for (i, c) in d.file.commits.iter().enumerate() {
        let ops = dsl::parse_script(&c.script).map_or(0, |s| s.len());
        println!(
            "{:>3}  {:<40} {ops} ops, {} lines",
            i + 1,
            format!("\"{}\"", c.message),
            c.script.lines().count()
        );
    }
    if !d.file.redo.is_empty() {
        println!("({} undone commits can be redone)", d.file.redo.len());
    }
    Ok(())
}

fn cmd_info(doc: &Path) -> Result<()> {
    let d = Document::load(doc)?;
    let state = d.render_head()?;
    println!(
        "canvas {}x{}, background {}",
        d.file.width, d.file.height, d.file.background
    );
    println!(
        "commits {} (redo {})",
        d.file.commits.len(),
        d.file.redo.len()
    );
    if state.vars.is_empty() {
        println!("variables: none");
    } else {
        println!("variables:");
        for (k, v) in &state.vars {
            println!("  ${k} = {v}");
        }
    }
    let current = state.current.map(|c| state.layers[c].name.as_str());
    println!(
        "layers (top first; current: {}):",
        current.unwrap_or("none")
    );
    for layer in state.layers.iter().rev() {
        let (coverage, bbox) = layer_stats(&layer.pixmap);
        let bbox = bbox.map_or("empty".to_string(), |(x0, y0, x1, y1)| {
            format!("bbox {x0},{y0}..{x1},{y1}")
        });
        println!(
            "  {:<16} {:<7} opacity {:<4} {:<11} coverage {:>5.1}%  {bbox}",
            layer.name,
            if layer.visible { "visible" } else { "hidden" },
            layer.opacity,
            blend_name(layer.blend),
            coverage * 100.0
        );
    }
    Ok(())
}

/// Mean alpha and bounding box (inclusive-exclusive) of non-transparent pixels.
fn layer_stats(p: &tiny_skia::Pixmap) -> (f32, Option<(u32, u32, u32, u32)>) {
    let w = p.width();
    let mut sum = 0u64;
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for (i, px) in p.data().as_chunks::<4>().0.iter().enumerate() {
        if px[3] == 0 {
            continue;
        }
        sum += px[3] as u64;
        let (x, y) = (i as u32 % w, i as u32 / w);
        bbox = Some(match bbox {
            None => (x, y, x + 1, y + 1),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
        });
    }
    let n = (p.width() as u64 * p.height() as u64).max(1);
    (sum as f32 / (255.0 * n as f32), bbox)
}

struct ViewOpts {
    crop: Option<[f32; 4]>,
    max: u32,
    grid: Option<Grid>,
    filter: LayerFilter,
    at: Option<usize>,
    alpha: bool,
    pixels: bool,
}

/// Highest scale a zoomed view re-renders at (as for `export --scale`).
const MAX_VIEW_SCALE: u32 = 16;
/// Cap (canvas px) up to which blur reach adds to the margin rendered
/// around a zoomed crop. Beyond it, measured differences from a full render
/// were a level or two at most.
const MAX_VIEW_MARGIN: u32 = 160;
/// Largest region render for a zoomed view, in device pixels per layer.
const VIEW_PIXEL_BUDGET: f32 = 8_000_000.0;

/// Margin (canvas px) and render scale for a zoomed crop that wants
/// `wanted` output px per canvas px.
///
/// The margin covers `reach`. Its soft part (blur tails, which only shift
/// values near the crop's edges a little) is capped and given up before
/// zoom; its hard part (marks that read a layer, flow fields: cutting those
/// changes whole marks anywhere in the crop) is not, so a small crop of
/// such a document renders at a lower scale instead.
fn view_margin_and_scale(crop: Crop, canvas: (u32, u32), reach: Reach, wanted: f32) -> (u32, u32) {
    let (cw, ch) = canvas;
    // The largest whole scale whose region fits the pixel budget.
    let max_scale = |margin: u32| {
        let w = (crop.x + crop.w + margin).min(cw) - crop.x.saturating_sub(margin);
        let h = (crop.y + crop.h + margin).min(ch) - crop.y.saturating_sub(margin);
        (VIEW_PIXEL_BUDGET / (w as f32 * h as f32))
            .sqrt()
            .floor()
            .max(1.0)
    };
    let hard = (reach.hard.ceil() as u32 + 8).max(16);
    let mut margin = (hard + reach.soft.ceil() as u32).min(hard.max(MAX_VIEW_MARGIN));
    while margin > hard && max_scale(margin) < wanted.ceil() {
        margin = margin.saturating_sub(8).max(hard);
    }
    (margin, wanted.min(max_scale(margin)).ceil() as u32)
}

fn cmd_view(doc: &Path, out: Option<PathBuf>, opts: &ViewOpts) -> Result<()> {
    if opts.max < 16 {
        bail!("--max must be at least 16");
    }
    let d = Document::load(doc)?;
    let at = opts.at.unwrap_or(d.file.commits.len());
    d.check_commit(at)?;
    let (cw, ch) = (d.file.width, d.file.height);
    let crop = match opts.crop {
        None => Crop {
            x: 0,
            y: 0,
            w: cw,
            h: ch,
        },
        Some([x, y, w, h]) => {
            let (x0, y0) = (x.max(0.0).floor(), y.max(0.0).floor());
            let (x1, y1) = ((x + w).min(cw as f32).ceil(), (y + h).min(ch as f32).ceil());
            if x1 <= x0 || y1 <= y0 {
                bail!("--crop lies outside the {cw}x{ch} canvas");
            }
            Crop {
                x: x0 as u32,
                y: y0 as u32,
                w: (x1 - x0) as u32,
                h: (y1 - y0) as u32,
            }
        }
    };
    let grid = opts.grid.unwrap_or(if opts.crop.is_some() {
        Grid::Auto
    } else {
        Grid::Off
    });
    let fit = opts.max as f32 / crop.w.max(crop.h) as f32;
    let (img, how) = if opts.pixels || fit <= 1.0 {
        let state = d.render_at(at, 1.0)?;
        let canvas = view_composite(&state, opts)?;
        let how = if fit > 1.0 { ", canvas pixels" } else { "" };
        (
            view::pixel_view(&canvas, crop, opts.max, grid),
            how.to_string(),
        )
    } else {
        // Zooming in: re-render just the crop, plus a margin wide enough
        // for marks, blurs and flow fields near its edges, at a higher
        // scale, so marks are drawn at the output resolution.
        let region = |margin: u32, scale| {
            Viewport::region(
                crop.x.saturating_sub(margin),
                crop.y.saturating_sub(margin),
                (crop.x + crop.w + margin).min(cw),
                (crop.y + crop.h + margin).min(ch),
                scale,
            )
        };
        let wanted = fit.min(MAX_VIEW_SCALE as f32);
        let (margin, scale) = view_margin_and_scale(crop, (cw, ch), d.influence_reach(at), wanted);
        let zoom = wanted.min(scale as f32);
        let viewport = region(margin, scale);
        let state = d.render_region(at, viewport)?;
        let device = view_composite(&state, opts)?;
        let rect = Crop {
            x: (crop.x - viewport.x as u32) * scale,
            y: (crop.y - viewport.y as u32) * scale,
            w: crop.w * scale,
            h: crop.h * scale,
        };
        (
            view::rendered_view(&device, rect, crop, zoom, grid),
            format!(", rendered at {scale}x"),
        )
    };
    let out = out.unwrap_or_else(|| doc.join("view.png"));
    img.pixmap
        .save_png(&out)
        .with_context(|| format!("writing {}", out.display()))?;
    let grid_note = img
        .grid
        .map_or(String::new(), |n| format!(", grid every {n} px"));
    println!(
        "wrote {} ({}x{}): canvas {},{} {}x{} at commit {at}, {} output px per canvas px{how}{grid_note}",
        out.display(),
        img.pixmap.width(),
        img.pixmap.height(),
        crop.x,
        crop.y,
        crop.w,
        crop.h,
        fmt_zoom(img.zoom)
    );
    Ok(())
}

fn fmt_zoom(z: f32) -> String {
    if (z - z.round()).abs() < 1e-4 {
        format!("{}", z.round())
    } else {
        format!("{z:.3}")
    }
}

/// Composite for a view, honouring the layer filter and `--alpha`.
fn view_composite(state: &RenderState, opts: &ViewOpts) -> Result<tiny_skia::Pixmap> {
    let mut canvas = state.composite(&opts.filter).map_err(|e| anyhow!(e))?;
    if opts.alpha {
        if opts.filter.solo.is_none() && canvas.pixels().iter().all(|p| p.alpha() == 255) {
            println!("note: the composite is fully opaque; use --alpha with --solo LAYER");
        }
        view::alpha_to_grey(&mut canvas);
    }
    Ok(canvas)
}

fn cmd_sample(doc: &Path, points: &[(f32, f32)], layer: Option<&str>, radius: f32) -> Result<()> {
    let d = Document::load(doc)?;
    let state = d.render_head()?;
    let owned;
    let pixmap = match layer {
        Some(name) => &state.layer_named(name).map_err(|e| anyhow!(e))?.pixmap,
        None => {
            owned = state
                .composite(&LayerFilter::default())
                .map_err(|e| anyhow!(e))?;
            &owned
        }
    };
    for &(x, y) in points {
        let Some(c) = raster::average_disc(pixmap, x, y, radius) else {
            println!(
                "{x},{y}  outside canvas ({}x{})",
                d.file.width, d.file.height
            );
            continue;
        };
        let (l, ch, h) = c.to_oklch();
        println!(
            "{x},{y}  {}  alpha {:.2}  oklch({l:.3} {ch:.3} {h:.0})",
            c.hex(),
            c.a
        );
    }
    Ok(())
}

fn cmd_import(doc: &Path, file: &Path, name: Option<String>) -> Result<()> {
    let d = Document::load(doc)?;
    let name = match name {
        Some(n) => n,
        None => file
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| {
                s.chars()
                    .map(|c| if dsl::is_name(&c.to_string()) { c } else { '-' })
                    .collect()
            })
            .ok_or_else(|| anyhow!("cannot derive a name from {}", file.display()))?,
    };
    if !dsl::is_name(&name) {
        bail!("bad asset name '{name}' (use letters, digits, '_' and '-')");
    }
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let img = tiny_skia::Pixmap::decode_png(&bytes)
        .with_context(|| format!("{} is not a PNG this tool can read", file.display()))?;
    fs::create_dir_all(d.assets_dir())?;
    let target = d.assets_dir().join(format!("{name}.png"));
    let replaced = target.exists();
    fs::write(&target, &bytes).with_context(|| format!("writing {}", target.display()))?;
    if replaced {
        // Earlier commits may draw this asset; their cached renders are stale.
        d.clear_cache();
    }
    println!(
        "imported {name} ({}x{}){}; use: image src={name}",
        img.width(),
        img.height(),
        if replaced {
            ", replacing the old asset"
        } else {
            ""
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn negative_sample_points_move_behind_double_dash() {
        let out = normalize_args(args(&[
            "easel", "sample", "d", "-5,3", "--radius", "3", "1,2",
        ]));
        assert_eq!(
            out,
            args(&["easel", "sample", "d", "--radius", "3", "--", "-5,3", "1,2"])
        );
        let plain = args(&["easel", "sample", "d", "1,2", "--radius", "3"]);
        assert_eq!(normalize_args(plain.clone()), plain);
        let view = args(&["easel", "view", "d", "--crop", "-5,3,10,10"]);
        assert_eq!(normalize_args(view.clone()), view);
    }

    #[test]
    fn view_margin_keeps_the_hard_reach() {
        let crop = Crop {
            x: 300,
            y: 400,
            w: 20,
            h: 20,
        };
        let canvas = (768, 1024);
        // Large flow marks: the whole hard reach stays, the zoom gives way.
        let big = Reach {
            hard: 180.0,
            soft: 40.0,
        };
        let (margin, scale) = view_margin_and_scale(crop, canvas, big, 16.0);
        assert!(margin >= 188, "margin {margin}");
        assert!(scale < 16);
        let side = (crop.w + 2 * margin) as f32 * scale as f32;
        assert!(side * side <= VIEW_PIXEL_BUDGET);
        // Only blur reach: that margin is given up first, keeping 16x.
        let soft = Reach {
            hard: 0.0,
            soft: 150.0,
        };
        assert_eq!(view_margin_and_scale(crop, canvas, soft, 16.0), (72, 16));
        // Plenty of budget: the full margin.
        let (margin, scale) = view_margin_and_scale(crop, canvas, big, 2.0);
        assert_eq!((margin, scale), (188, 2), "no soft margin beyond the cap");
    }

    #[test]
    fn quiet_report_keeps_only_short_mark_ops() {
        assert!(notable_report(
            "4   stipple -> 0 marks (shape is outside the canvas)"
        ));
        assert!(notable_report(
            "5   stipple -> 90 marks (10 of 100 not placed: the shape fills only 2.0% of its bbox)"
        ));
        assert!(!notable_report("6   stipple -> 10 marks"));
        assert!(!notable_report("1   set $outside"));
        assert!(!notable_report(
            "2   layer outside (new; opacity 1, normal, position 2 of 2)"
        ));
        assert!(!notable_report("3   rename-layer a -> not placed"));
    }

    #[test]
    fn view_at_a_missing_commit_is_an_error() {
        let dir = std::env::temp_dir().join(format!("easel-test-{}-view-at", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut d = Document::create(&dir, 40, 30, "white").unwrap();
        d.apply("dot at=10,10 r=3 color=black", "dot", false)
            .unwrap();
        let opts = ViewOpts {
            crop: None,
            max: 1000,
            grid: None,
            filter: LayerFilter::default(),
            at: Some(3),
            alpha: false,
            pixels: false,
        };
        let err = cmd_view(&dir, Some(dir.join("v.png")), &opts).unwrap_err();
        assert!(err.to_string().contains("commit 3 does not exist"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
