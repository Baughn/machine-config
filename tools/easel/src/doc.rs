//! The on-disk document (`doc.json`), rendering by replay, and the render cache.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

use crate::color::Rgba;
use crate::dsl;
use crate::render::{self, Layer, LayerMeta, RenderState, Viewport};

const FORMAT: u32 = 1;
/// Bump whenever rendering output changes, to invalidate old caches.
const RENDER_VERSION: u32 = 4;
/// How many cached snapshots to keep (head, plus a couple for undo/--at).
const CACHE_SLOTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub message: String,
    pub script: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocFile {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub background: String,
    pub commits: Vec<Commit>,
    pub redo: Vec<Commit>,
}

/// How far (canvas px) a region render's margin should reach: `hard` for
/// mark lookups and flow fields, where too little margin changes whole
/// marks, `soft` for blur tails, where it shifts values slightly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    pub hard: f32,
    pub soft: f32,
}

/// A loaded document directory.
pub struct Document {
    pub dir: PathBuf,
    pub file: DocFile,
}

/// Cached render state metadata (`cache/<hash>/state.json`).
#[derive(Serialize, Deserialize)]
struct CacheState {
    hash: String,
    commits: usize,
    width: u32,
    height: u32,
    vars: BTreeMap<String, String>,
    current: Option<usize>,
    layers: Vec<LayerMeta>,
}

/// FNV-1a, 64 bit: a stable hash for cache keys.
fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Write `contents` to `path` atomically and durably: a per-process temp
/// file is written and fsynced, renamed over `path`, then the directory is
/// fsynced so the rename itself survives a crash.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", tmp.display()));
    }
    fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    if let Some(dir) = path.parent() {
        // Directory fsync is best effort: not every platform allows it.
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

impl Document {
    /// Create a new document directory.
    ///
    /// # Errors
    ///
    /// Fails if the directory already contains a document, the size is out
    /// of range, the background is not a colour, or on I/O errors.
    pub fn create(dir: &Path, width: u32, height: u32, background: &str) -> Result<Document> {
        if !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
            bail!("canvas size must be between 1 and 16384 px per side");
        }
        render::parse_background(background).map_err(|e| anyhow::anyhow!("--bg: {e}"))?;
        if dir.join("doc.json").exists() {
            bail!("{} already contains a document", dir.display());
        }
        fs::create_dir_all(dir.join("assets"))
            .with_context(|| format!("creating {}", dir.display()))?;
        let doc = Document {
            dir: dir.to_path_buf(),
            file: DocFile {
                format: FORMAT,
                width,
                height,
                background: background.to_string(),
                commits: Vec::new(),
                redo: Vec::new(),
            },
        };
        doc.save()?;
        Ok(doc)
    }

    /// Load a document directory.
    ///
    /// # Errors
    ///
    /// Fails if `doc.json` is missing, unreadable or of an unknown format.
    pub fn load(dir: &Path) -> Result<Document> {
        let path = dir.join("doc.json");
        let text = fs::read_to_string(&path)
            .with_context(|| format!("{} is not an easel document (no doc.json)", dir.display()))?;
        let file: DocFile =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if file.format != FORMAT {
            bail!("unsupported document format {}", file.format);
        }
        Ok(Document {
            dir: dir.to_path_buf(),
            file,
        })
    }

    /// Persist `doc.json` atomically.
    ///
    /// # Errors
    ///
    /// Fails on serialisation or I/O errors.
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.file)?;
        write_atomic(&self.dir.join("doc.json"), json.as_bytes())
    }

    pub fn background(&self) -> Result<Rgba> {
        render::parse_background(&self.file.background).map_err(|e| anyhow::anyhow!(e))
    }

    fn cache_dir(&self) -> PathBuf {
        self.dir.join("cache")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.dir.join("assets")
    }

    /// Chain hashes: `h[0]` covers the canvas, `h[i]` also commits `1..=i`.
    fn prefix_hashes(&self, upto: usize) -> Vec<u64> {
        let header = format!(
            "easel/{RENDER_VERSION}/{}x{}/{}",
            self.file.width, self.file.height, self.file.background
        );
        let mut hashes = vec![fnv1a(0xcbf2_9ce4_8422_2325, header.as_bytes())];
        for commit in &self.file.commits[..upto] {
            let prev = *hashes.last().expect("non-empty");
            hashes.push(fnv1a(prev, commit.script.as_bytes()));
        }
        hashes
    }

    /// A blank render state for this document at `scale`.
    pub fn blank_state(&self, scale: f32) -> Result<RenderState> {
        Ok(RenderState::new(
            self.file.width,
            self.file.height,
            self.background()?,
            scale,
            self.assets_dir(),
        ))
    }

    /// Replay commits `from+1..=to` onto `state`.
    ///
    /// # Errors
    ///
    /// Fails if a stored commit no longer runs (e.g. a missing asset).
    pub fn replay(&self, state: &mut RenderState, from: usize, to: usize) -> Result<()> {
        for (idx, commit) in self.file.commits[from..to].iter().enumerate() {
            let number = from + idx + 1;
            let stmts = dsl::parse_script(&commit.script)
                .map_err(|e| anyhow::anyhow!("commit {number}: {e}"))?;
            state
                .run(&stmts, number)
                .map_err(|e| anyhow::anyhow!("commit {number}: {e}"))?;
        }
        Ok(())
    }

    /// Fails unless commit `n` exists (0, the blank canvas, always does).
    ///
    /// # Errors
    ///
    /// `n` is beyond the last commit.
    pub fn check_commit(&self, n: usize) -> Result<()> {
        if n > self.file.commits.len() {
            bail!(
                "commit {n} does not exist (the document has {})",
                self.file.commits.len()
            );
        }
        Ok(())
    }

    /// Render the state after commit `n` (0 = blank) at `scale`.
    /// At scale 1 the cache is used and refreshed.
    ///
    /// # Errors
    ///
    /// Fails if `n` is out of range or replay fails.
    pub fn render_at(&self, n: usize, scale: f32) -> Result<RenderState> {
        self.check_commit(n)?;
        if scale != 1.0 {
            let mut state = self.blank_state(scale)?;
            self.replay(&mut state, 0, n)?;
            return Ok(state);
        }
        let hashes = self.prefix_hashes(n);
        let cached = (1..=n)
            .rev()
            .find_map(|k| self.load_cache(hashes[k], k).map(|s| (k, s)));
        let (start, mut state) = match cached {
            Some(found) => found,
            None => (0, self.blank_state(1.0)?),
        };
        self.replay(&mut state, start, n)?;
        if n > start {
            // A failed cache write only costs speed next time.
            let _ = self.store_cache(&state, n);
        }
        Ok(state)
    }

    /// Roughly how far (canvas px) pixels are influenced by content
    /// elsewhere in commits `1..=n`, which a region render needs as margin
    /// to match the full render near its edges.
    ///
    /// Marks are placed in canvas coordinates and drawn whether or not
    /// their centre is in the region, so only raster lookups need margin:
    /// the largest extent of a mark that reads a layer (pointillize,
    /// `weight=`, `clip=`) plus the largest flow-field reach (three sigma)
    /// is `hard`; the largest blur/feather reach (two sigma) is `soft`.
    pub fn influence_reach(&self, n: usize) -> Reach {
        let (mut marks, mut flow, mut blur) = (0f32, 0f32, 0f32);
        let mut vars = BTreeMap::new();
        for commit in &self.file.commits[..n] {
            let Ok(stmts) = dsl::parse_script(&commit.script) else {
                continue;
            };
            for stmt in &stmts {
                if stmt.op == "set" {
                    for (k, raw) in &stmt.args {
                        if let Ok(v) = dsl::substitute(raw, &vars) {
                            vars.insert(k.clone(), v);
                        }
                    }
                    continue;
                }
                let Ok(args) = dsl::Args::new(stmt, &vars) else {
                    continue;
                };
                let max_of = |key: &str, default: f32| {
                    args.ramp(key).ok().flatten().map_or(default, |v| v.max())
                };
                // Gaussian sigma is value/2, so two sigma is the value.
                blur = blur.max(max_of("feather", 0.0));
                match stmt.op.as_str() {
                    "blur" => blur = blur.max(max_of("r", 0.0)),
                    "dot" | "stipple" | "pointillize" | "dabs" => {
                        let (r, aspect) = (max_of("r", 2.0), max_of("aspect", 2.5).max(1.0));
                        let reads =
                            stmt.op == "pointillize" || args.has("weight") || args.has("clip");
                        if reads {
                            let extent = match args.raw("mark").unwrap_or("circle") {
                                "square" => r * std::f32::consts::SQRT_2,
                                "dab" | "line" => r * aspect,
                                _ => r,
                            };
                            marks = marks.max(extent + max_of("soft", 0.0));
                        }
                        if args.raw("angle") == Some("flow") {
                            // Three times the flow field's blur sigma (see
                            // marks.rs), whatever the mark kind: angles turn
                            // on small differences, so two was not enough.
                            flow = flow.max(3.0 * (r * aspect).max(2.0));
                        }
                    }
                    _ => {}
                }
            }
        }
        Reach {
            hard: marks + flow,
            soft: blur,
        }
    }

    /// Render the state after commit `n` for `view` only (a region of the
    /// canvas at some scale), replaying from scratch: the cache holds
    /// full-canvas scale-1 states only.
    ///
    /// # Errors
    ///
    /// Fails if `n` is out of range or replay fails.
    pub fn render_region(&self, n: usize, view: Viewport) -> Result<RenderState> {
        self.check_commit(n)?;
        let mut state = RenderState::with_viewport(
            self.file.width,
            self.file.height,
            self.background()?,
            view,
            self.assets_dir(),
        );
        self.replay(&mut state, 0, n)?;
        Ok(state)
    }

    /// Render the current head at scale 1.
    pub fn render_head(&self) -> Result<RenderState> {
        self.render_at(self.file.commits.len(), 1.0)
    }

    fn load_cache(&self, hash: u64, commits: usize) -> Option<RenderState> {
        let dir = self.cache_dir().join(format!("{hash:016x}"));
        let meta_path = dir.join("state.json");
        let meta: CacheState = serde_json::from_slice(&fs::read(&meta_path).ok()?).ok()?;
        if meta.hash != format!("{hash:016x}")
            || meta.commits != commits
            || meta.width != self.file.width
            || meta.height != self.file.height
        {
            return None;
        }
        let size = tiny_skia::IntSize::from_wh(self.file.width, self.file.height)?;
        let mut layers = Vec::with_capacity(meta.layers.len());
        for (i, lm) in meta.layers.into_iter().enumerate() {
            let bytes = fs::read(dir.join(format!("layer-{i}.raw"))).ok()?;
            let pixmap = Pixmap::from_vec(bytes, size)?;
            layers.push(Layer {
                blend: render::blend_by_name(&lm.blend)?,
                name: lm.name,
                opacity: lm.opacity,
                visible: lm.visible,
                pixmap,
            });
        }
        // Mark as recently used for the LRU.
        if let Ok(f) = fs::File::options().append(true).open(&meta_path) {
            let _ = f.set_modified(SystemTime::now());
        }
        let mut state = self.blank_state(1.0).ok()?;
        state.vars = meta.vars;
        state.current = meta.current.filter(|&c| c < layers.len());
        state.layers = layers;
        Some(state)
    }

    fn store_cache(&self, state: &RenderState, commits: usize) -> Result<()> {
        let hash = *self.prefix_hashes(commits).last().expect("non-empty");
        let root = self.cache_dir();
        let dir = root.join(format!("{hash:016x}"));
        let meta_path = dir.join("state.json");
        let _ = fs::remove_file(&meta_path);
        fs::create_dir_all(&dir)?;
        for (i, layer) in state.layers.iter().enumerate() {
            fs::write(dir.join(format!("layer-{i}.raw")), layer.pixmap.data())?;
        }
        let meta = CacheState {
            hash: format!("{hash:016x}"),
            commits,
            width: self.file.width,
            height: self.file.height,
            vars: state.vars.clone(),
            current: state.current,
            layers: state.layers.iter().map(Layer::meta).collect(),
        };
        write_atomic(&meta_path, &serde_json::to_vec(&meta)?)?;
        self.prune_cache(&root)
    }

    /// Keep only the most recently used snapshots.
    fn prune_cache(&self, root: &Path) -> Result<()> {
        let mut entries: Vec<(SystemTime, PathBuf)> = fs::read_dir(root)?
            .filter_map(|e| e.ok())
            .map(|e| {
                let t = fs::metadata(e.path().join("state.json"))
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (t, e.path())
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (_, path) in entries.into_iter().skip(CACHE_SLOTS) {
            let _ = fs::remove_dir_all(path);
        }
        Ok(())
    }

    /// Drop every cached snapshot (e.g. after an asset changes).
    pub fn clear_cache(&self) {
        let _ = fs::remove_dir_all(self.cache_dir());
    }

    /// Apply a script as a new commit (or just render it with `dry_run`).
    ///
    /// Returns the per-op report lines.
    ///
    /// # Errors
    ///
    /// Fails without changing the document if any line fails.
    pub fn apply(&mut self, script: &str, message: &str, dry_run: bool) -> Result<Vec<String>> {
        let stmts = dsl::parse_script(script).map_err(|e| anyhow::anyhow!(e))?;
        if stmts.is_empty() {
            bail!("script contains no ops");
        }
        let mut state = self.render_head()?;
        let number = self.file.commits.len() + 1;
        let report = state.run(&stmts, number).map_err(|e| anyhow::anyhow!(e))?;
        if !dry_run {
            self.file.commits.push(Commit {
                message: message.to_string(),
                script: script.to_string(),
            });
            self.file.redo.clear();
            self.save()?;
            let _ = self.store_cache(&state, number);
        }
        Ok(report)
    }

    /// Replace the script of commit `n` (1-based) and replay the commits
    /// after it, which keep their own scripts and seeds. Discards the redo
    /// stack.
    ///
    /// Returns the per-op report lines of the new commit `n`.
    ///
    /// # Errors
    ///
    /// Fails without changing the document if the new script or any later
    /// commit fails.
    pub fn amend(&mut self, n: usize, script: &str, message: Option<&str>) -> Result<Vec<String>> {
        let total = self.file.commits.len();
        if n == 0 || n > total {
            bail!("no commit {n} (1..={total})");
        }
        let stmts = dsl::parse_script(script).map_err(|e| anyhow::anyhow!(e))?;
        if stmts.is_empty() {
            bail!("script contains no ops");
        }
        let mut state = self.render_at(n - 1, 1.0)?;
        let report = state.run(&stmts, n).map_err(|e| anyhow::anyhow!(e))?;
        let message =
            message.map_or_else(|| self.file.commits[n - 1].message.clone(), str::to_string);
        let old = std::mem::replace(
            &mut self.file.commits[n - 1],
            Commit {
                message,
                script: script.to_string(),
            },
        );
        if let Err(e) = self.replay(&mut state, n, total) {
            self.file.commits[n - 1] = old;
            bail!("{e:#} (after amending commit {n}); nothing changed");
        }
        // Undone commits were recorded against the old history and may no
        // longer run on the new one; as after an apply, they are dropped.
        self.file.redo.clear();
        self.save()?;
        let _ = self.store_cache(&state, total);
        Ok(report)
    }

    /// Move the last `n` commits to the redo stack.
    ///
    /// # Errors
    ///
    /// Fails if there are fewer than `n` commits.
    pub fn undo(&mut self, n: usize) -> Result<()> {
        if n > self.file.commits.len() {
            bail!("cannot undo {n}: only {} commits", self.file.commits.len());
        }
        for _ in 0..n {
            let c = self.file.commits.pop().expect("length checked");
            self.file.redo.push(c);
        }
        self.save()
    }

    /// Move `n` commits back from the redo stack.
    ///
    /// # Errors
    ///
    /// Fails if fewer than `n` commits can be redone.
    pub fn redo(&mut self, n: usize) -> Result<()> {
        if n > self.file.redo.len() {
            bail!(
                "cannot redo {n}: only {} undone commits",
                self.file.redo.len()
            );
        }
        for _ in 0..n {
            let c = self.file.redo.pop().expect("length checked");
            self.file.commits.push(c);
        }
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("easel-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    const SCRIPT_A: &str = "layer name=under visible=false\n\
        fill in=canvas color=#806040\n\
        gradient in=circle(40,30,25) stops=#f0c090@0,#402010@1 center=40,30 radius=25\n\
        layer name=paint\n\
        pointillize source=under density=40 r=1..2 mark=dab angle=flow jitter=0.03,0.01,8\n";
    const SCRIPT_B: &str = "set ink=oklch(0.3,0.05,260)\n\
        stipple in=ellipse(40,30,30,20) count=300 color=$ink weight=dark(under) r=0.5..1.2\n\
        dabs path=5,5;40,50;75,10 spacing=2 r=1.5 color=#c33 scatter=1 taper=0.8 \
        mark=line angle=follow\n\
        stroke path=0,60;80,0 width=2 color=#123 taper=0.5\n";

    fn pixels(state: &RenderState) -> Vec<u8> {
        state
            .composite(&render::LayerFilter::default())
            .unwrap()
            .data()
            .to_vec()
    }

    #[test]
    fn determinism_cache_and_undo() {
        let dir = scratch("determinism");
        let mut doc = Document::create(&dir, 80, 60, "#101018").unwrap();
        doc.apply(SCRIPT_A, "a", false).unwrap();
        doc.apply(SCRIPT_B, "b", false).unwrap();
        let cached = pixels(&doc.render_head().unwrap());

        // Full replay from scratch matches the cached render.
        doc.clear_cache();
        let replayed = pixels(&doc.render_head().unwrap());
        assert_eq!(cached, replayed);
        assert!(cached.chunks(4).any(|p| p != cached[..4].as_ref()));

        // A second document with the same log is pixel-identical.
        let dir2 = scratch("determinism2");
        let mut doc2 = Document::create(&dir2, 80, 60, "#101018").unwrap();
        doc2.apply(SCRIPT_A, "a", false).unwrap();
        doc2.apply(SCRIPT_B, "b", false).unwrap();
        assert_eq!(pixels(&doc2.render_head().unwrap()), cached);

        // Undo, then re-apply the same script: identical again.
        let after_a = pixels(&doc.render_at(1, 1.0).unwrap());
        doc.undo(1).unwrap();
        assert_eq!(doc.file.redo.len(), 1);
        assert_eq!(pixels(&doc.render_head().unwrap()), after_a);
        doc.redo(1).unwrap();
        assert_eq!(pixels(&doc.render_head().unwrap()), cached);
        doc.undo(1).unwrap();
        doc.apply(SCRIPT_B, "b again", false).unwrap();
        assert!(doc.file.redo.is_empty());
        assert_eq!(pixels(&doc.render_head().unwrap()), cached);

        // --at 0 is the blank canvas.
        let blank = pixels(&doc.render_at(0, 1.0).unwrap());
        assert!(blank.chunks(4).all(|p| p == [16, 16, 24, 255]));
        assert!(doc.render_at(3, 1.0).is_err());
        assert!(doc.undo(5).is_err());
        assert!(doc.redo(1).is_err());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }

    #[test]
    fn failed_apply_changes_nothing() {
        let dir = scratch("atomic");
        let mut doc = Document::create(&dir, 40, 40, "white").unwrap();
        doc.apply("fill in=canvas color=#888", "base", false)
            .unwrap();
        let err = doc
            .apply(
                "fill in=canvas color=#f00\nstipple in=canvas count=5 color=#000 clip=nope",
                "x",
                false,
            )
            .unwrap_err();
        assert!(err.to_string().starts_with("line 2: stipple:"), "{err}");
        let reloaded = Document::load(&dir).unwrap();
        assert_eq!(reloaded.file.commits.len(), 1);
        let px = pixels(&reloaded.render_head().unwrap());
        assert_eq!(&px[..4], &[0x88, 0x88, 0x88, 255]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn amend_replays_later_commits() {
        let dir = scratch("amend");
        let mut doc = Document::create(&dir, 40, 30, "white").unwrap();
        doc.apply(
            "layer name=under visible=false\nfill in=canvas color=#f00",
            "a",
            false,
        )
        .unwrap();
        doc.apply(
            "layer name=paint\npointillize source=under count=50 r=2",
            "b",
            false,
        )
        .unwrap();
        doc.amend(
            1,
            "layer name=under visible=false\nfill in=canvas color=#00f",
            None,
        )
        .unwrap();
        let reloaded = Document::load(&dir).unwrap();
        assert_eq!(reloaded.file.commits[0].message, "a");
        // Commit 2 re-sampled the amended underpainting.
        let px = pixels(&reloaded.render_head().unwrap());
        assert!(px.chunks(4).any(|p| p[2] > 200 && p[0] < 50), "blue marks");
        // A change that breaks a later commit is refused and changes nothing.
        let err = doc.amend(1, "fill in=canvas color=#0f0", None).unwrap_err();
        assert!(err.to_string().contains("nothing changed"), "{err}");
        assert_eq!(Document::load(&dir).unwrap().file, reloaded.file);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn amend_discards_redo() {
        let dir = scratch("amend-redo");
        let mut doc = Document::create(&dir, 40, 30, "white").unwrap();
        doc.apply(
            "layer name=under visible=false\nfill in=canvas color=#f80",
            "a",
            false,
        )
        .unwrap();
        doc.apply("layer name=paint\ndot at=5,5 color=#00f", "b", false)
            .unwrap();
        doc.apply("pointillize source=under count=30 r=2", "c", false)
            .unwrap();
        doc.undo(1).unwrap();
        // Commit 3, undone, reads 'under', which the amended commit 1 no
        // longer creates: redoing it would leave a document that fails to
        // render, so the amend drops it.
        doc.amend(1, "fill in=canvas color=#0f0", None).unwrap();
        assert!(doc.file.redo.is_empty());
        assert!(doc.redo(1).is_err());
        assert!(Document::load(&dir).unwrap().render_head().is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dabs_close_up_round_shapes() {
        let dir = scratch("closed-dabs");
        let mut doc = Document::create(&dir, 100, 100, "white").unwrap();
        doc.apply(
            "dabs path=circle(50,50,30) count=4 r=4 color=#000 alpha=0.5\n\
             dabs path=rect(10,10,80,80) spacing=81 r=2 color=#000",
            "rings",
            false,
        )
        .unwrap();
        let state = doc.render_head().unwrap();
        let px = |x, y| state.layers[0].pixmap.pixel(x, y).unwrap().alpha();
        // Four dabs a quarter turn apart, none doubled at the start.
        let quarter = [px(80, 50), px(50, 80), px(20, 50), px(50, 20)];
        assert!(
            quarter
                .iter()
                .all(|&a| a == quarter[0] && a > 100 && a < 150),
            "{quarter:?}"
        );
        // Spacing 81 round a 320 px outline: four even steps, one per corner.
        assert!([px(10, 10), px(90, 10), px(90, 90), px(10, 90)]
            .iter()
            .all(|&a| a > 200));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn region_with_hard_margin_matches_large_flow_marks() {
        let dir = scratch("region-flow");
        let mut doc = Document::create(&dir, 200, 150, "#203040").unwrap();
        doc.apply(
            "layer name=under visible=false\nfill in=canvas color=#000\n\
             fill in=rect(0,0,100,150) color=#fff\nfill in=rect(20,0,10,150) color=#000\n\
             fill in=rect(160,0,10,150) color=#fff\nlayer name=paint\n\
             pointillize source=under density=6 r=6..9 mark=dab aspect=2..3 angle=flow",
            "flow",
            false,
        )
        .unwrap();
        let reach = doc.influence_reach(1);
        let margin = reach.hard.ceil() as u32 + 8;
        let scale = 4;
        let full = doc.render_at(1, scale as f32).unwrap();
        let (cx, cy, cw, ch) = (90u32, 60u32, 20u32, 20u32);
        let view = Viewport::region(
            cx.saturating_sub(margin),
            cy.saturating_sub(margin),
            (cx + cw + margin).min(200),
            (cy + ch + margin).min(150),
            scale,
        );
        let region = doc.render_region(1, view).unwrap();
        let (a, b) = (&full.layers[0].pixmap, &region.layers[0].pixmap);
        for y in cy * scale..(cy + ch) * scale {
            for x in cx * scale..(cx + cw) * scale {
                let bx = x - view.x as u32 * scale;
                let by = y - view.y as u32 * scale;
                assert_eq!(a.pixel(x, y), b.pixel(bx, by), "device {x},{y}");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn region_render_matches_full_render() {
        let dir = scratch("region");
        let mut doc = Document::create(&dir, 120, 90, "#101018").unwrap();
        doc.apply(SCRIPT_A, "a", false).unwrap();
        doc.apply(SCRIPT_B, "b", false).unwrap();
        doc.apply("layer name=glow blend=screen\nfill in=circle(60,40,20) color=#48c feather=6\n\
                   blur r=3 in=rect(30,20,60,40)\nstroke path=circle(60,40,25) width=1.5->4 color=#fff alpha=1->0.3",
                  "c", false)
            .unwrap();
        let full = doc.render_at(3, 3.0).unwrap();
        let full = full.composite(&render::LayerFilter::default()).unwrap();
        let view = Viewport::region(30, 20, 100, 80, 3);
        let region = doc.render_region(3, view).unwrap();
        let region = region.composite(&render::LayerFilter::default()).unwrap();
        // Compare the inner part, 20 canvas px from the region's own edges.
        for y in 60..120 {
            for x in 60..150 {
                let a = full.pixel(x + 90, y + 60).unwrap();
                let b = region.pixel(x, y).unwrap();
                assert_eq!(a, b, "device {x},{y} of the region");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clip_masks_non_mark_ops_and_copy_layer_copies() {
        let dir = scratch("clip");
        let mut doc = Document::create(&dir, 40, 20, "white").unwrap();
        let script = "layer name=mask visible=false\nfill in=rect(0,0,20,20) color=#fff\n\
                      layer name=paint\nfill in=canvas color=#f00 clip=mask\n\
                      copy-layer name=paint to=copy\nclear in=rect(0,0,10,20) clip=mask\n";
        doc.apply(script, "clip", false).unwrap();
        let state = doc.render_head().unwrap();
        let alpha = |layer: &str, x: u32| {
            state
                .layer_named(layer)
                .unwrap()
                .pixmap
                .pixel(x, 10)
                .unwrap()
                .alpha()
        };
        assert_eq!(
            (alpha("paint", 15), alpha("paint", 30)),
            (255, 0),
            "clipped fill"
        );
        assert_eq!(
            alpha("copy", 5),
            0,
            "clear clipped to both in= and the mask"
        );
        assert_eq!(alpha("copy", 15), 255);
        assert!(state.layer_named("copy").unwrap().visible);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scale_two_export_dimensions() {
        let dir = scratch("scale");
        let mut doc = Document::create(&dir, 50, 30, "white").unwrap();
        doc.apply("stipple in=canvas count=100 color=black r=1", "dots", false)
            .unwrap();
        let state = doc.render_at(1, 2.0).unwrap();
        let out = state.composite(&render::LayerFilter::default()).unwrap();
        assert_eq!((out.width(), out.height()), (100, 60));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scale_two_scales_every_mark_once() {
        let dir = scratch("scale-marks");
        let mut doc = Document::create(&dir, 50, 50, "white").unwrap();
        let script = "dot at=10,10 r=3 color=black\n\
                      stroke path=0,30;50,30 width=4 color=black smooth=false cap=butt\n\
                      fill in=rect(40,0,10,10) color=black\n";
        doc.apply(script, "marks", false).unwrap();
        let img = doc
            .render_at(1, 2.0)
            .unwrap()
            .composite(&render::LayerFilter::default())
            .unwrap();
        let dark = |x: u32, y: u32| img.pixel(x, y).unwrap().red() < 128;
        assert!(dark(20, 20), "dot centre");
        assert!(!dark(20, 28), "dot radius is 6 device px");
        assert!(dark(20, 25));
        let column: u32 = (0..100).filter(|&y| dark(50, y)).count() as u32;
        assert!(
            (7..=9).contains(&column),
            "stroke is {column} px wide, want 8"
        );
        assert!(dark(85, 5) && !dark(75, 5), "fill scaled");
        let _ = fs::remove_dir_all(&dir);
    }
}
