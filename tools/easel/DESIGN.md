# easel — a raster painting tool for language models

Last updated: 2026-09-29 (sharp zoomed crops, clip= masks, path shapes and ramps, amend, copy-layer)

`easel` is a command-line painting tool built for an AI painter that works
through a shell and sees only by reading PNG files. It favours pointillism —
many small marks with controlled colour variation — over line art.

The painter's loop is: write a small batch of paint operations, `apply` it,
`view` the result (whole canvas or a zoomed crop with a coordinate grid),
judge, and either continue or `undo`. Everything in this document serves that
loop.

## Principles

1. **Seeing is the primary feature.** The painter is blind between renders.
   `view` must produce images that are legible when read back at ~1000 px:
   crops, zoom, coordinate grid with numeric labels, layer solo/hide, history
   states.
2. **The painting is a program.** A document is an ordered log of commits,
   each holding the DSL text that was applied. Rendering replays the log with
   deterministic seeded randomness. That gives undo/redo for free, makes every
   painting reproducible, and makes export resolution-independent
   (`export --scale 3` re-renders every mark at 3x).
3. **Batch, not chatty.** One `apply` of a multi-line script is one undo step.
   Applies are atomic: if any line fails to parse or validate, nothing is
   committed and the error names the line.
4. **Text feedback where possible.** `apply` reports what each op did (marks
   placed, marks rejected by clip). `info` and `sample` let the painter reason
   about the canvas without spending an image read.

## Document format

A document is a directory, conventionally named `*.easel`:

```
portrait.easel/
  doc.json      # metadata + commit log + redo stack (source of truth)
  assets/       # images imported with `easel import` (referenced by name)
  cache/        # rendered head state; safe to delete at any time
  view.png      # default output of `easel view`
```

`doc.json`:

```json
{
  "format": 1,
  "width": 768, "height": 1024,
  "background": "#101018",
  "commits": [ { "message": "block in face", "script": "layer name=under ...\n..." } ],
  "redo":    [ { "message": "...", "script": "..." } ]
}
```

- `apply` appends a commit and clears `redo` (and says so when it was not
  empty).
- `amend N` replaces commit N's script and replays the later commits, which
  keep their scripts and seeds; if any of them fails nothing changes. Like
  `apply`, it clears `redo`: undone commits were recorded against the old
  history and could fail on the new one, leaving a document that no
  longer renders.
- `undo [N]` moves the last N commits to `redo`; `redo [N]` moves them back.
- `cache/` holds the rendered layer rasters, render-state (variables, layer
  metadata, current layer) and a hash of (width, height, background, all
  commit scripts). If the hash matches, `apply`/`view` start from the cache;
  otherwise they replay from scratch. `undo` simply replays (or invalidates).
  Writes to doc.json go via write-to-temp + fsync + rename + directory
  fsync; the temp name carries the process id.

### Determinism

Every random choice comes from a ChaCha RNG. The seed for the op on line `L`
of commit `C` is `hash(C, L)` unless the op has an explicit `seed=`.
Re-applying the same script at the same position yields identical pixels.
Rendering at `scale = s` multiplies all coordinates and lengths by `s` and
uses the same random sequence, so export at 2x is the same painting, sharper.

## Rendering model

- Canvas coordinates are floating point pixels, origin top-left, y down.
- Each layer is an RGBA premultiplied raster (tiny-skia `Pixmap`) at
  `width*scale x height*scale`.
- Layer properties: `name`, `opacity` (0..1), `blend` mode, `visible`.
- Composite = background colour, then visible layers bottom to top with their
  blend mode and opacity.
- Blend modes: `normal multiply screen overlay darken lighten add color-dodge
  color-burn soft-light hard-light difference hue saturation color luminosity`
  (the tiny-skia set, `add` = Plus).
- Anti-aliasing always on.

## CLI

All commands take the document path first. Output is terse, plain text,
suited to being read by a model.

```
easel new DOC --size WxH [--bg COLOR]
easel apply DOC [-m MSG] [--dry-run] [-q] [FILE]    # FILE omitted or "-" = stdin
easel amend DOC N [-m MSG] [-q] [FILE]  # replace commit N, replay the rest
easel undo DOC [N]            # default 1
easel redo DOC [N]
easel log DOC                  # index, message, op count, line count
easel show DOC N               # print the script of commit N (header on stderr)
easel info DOC                 # size, commit count, variables, layers
                               #   (name, visible, opacity, blend, coverage %, bbox)
easel view DOC [-o OUT] [--crop X,Y,W,H] [--max PX] [--grid N|auto|off]
               [--solo LAYER] [--hide L1,L2] [--show L1,L2] [--at N]
               [--alpha]       # show layer alpha as greyscale (useful with --solo)
               [--pixels]      # zoom by repeating 1x pixels, not re-rendering
easel sample DOC X,Y [X,Y ...] [--layer NAME] [--radius R]   # negative X,Y ok
easel export DOC -o OUT.png [--scale S] [--hide L1,L2] [--show L1,L2]
easel import DOC FILE [--name NAME]   # copy an image into assets/
easel ops [OP]                 # DSL reference (all ops, or one op in detail)
```

### `view` details

- Default output `DOC/view.png`; prints the path, the canvas region shown and
  the pixel scale so the painter knows how to map back to canvas coordinates.
- `--crop X,Y,W,H` shows a canvas region; the output is scaled so the long
  side is `--max` (default 1000). Zooming in re-renders: the document is
  replayed at scale `ceil(zoom)` for the crop region only, so marks are as
  sharp as in an export. Zooming out uses a box filter. `--pixels` instead
  enlarges the scale-1 canvas pixels by a whole number (nearest neighbour).
- `--grid N` draws lines every N canvas px, `auto` picks a round number giving
  roughly 6-12 lines across the view (default: `auto` for crops, `off` for
  full-canvas views unless requested). Lines are thin, semi-transparent, drawn
  in a colour that contrasts with the local image (e.g. difference blend or a
  dark/light double line). Every line is labelled with its canvas coordinate
  using a small built-in bitmap font (digits, `-`, `,`, `x`, `y`; scaled 2x so
  it survives downsampling), labels on the top and left edges with a backing
  box for legibility.
- `--at N` renders the state after commit N (0 = blank canvas).
- `--solo LAYER` shows only that layer (hidden layers included) over a neutral
  checkerboard; `--alpha` shows alpha as greyscale.

### `sample`

Prints, per point, the composited colour (or the named layer's colour) as
`#rrggbb`, alpha, and `oklch(L C H)`. `--radius R` averages the on-canvas
part of a disc. A point (or disc) entirely off the canvas prints
`outside canvas (WxH)` instead of a colour.

### `apply` output

One line per op: line number, op name, and a short result, e.g.

```
3  layer under (new, hidden)
4  fill ellipse -> 1 shape
7  stipple -> 4000 marks (812 rejected by clip)
ok: commit 5 "block in face": 3 ops, render 0.41s
```

Errors: `error: line 7: stipple: unknown key 'raduis' (did you mean 'r'?)`.
Unknown keys, missing required keys, bad values are all errors — never
silently ignored.

## DSL

Line-based. `#` starts a comment (outside parentheses/quotes). Blank lines
ignored. A line ending in `\` continues on the next line. Each line is:

```
OPNAME key=value key=value ...
```

Tokens are separated by whitespace, except whitespace inside parentheses,
so `color=oklch(0.7, 0.1, 40)` is one token. Keys are kebab-case.

### Value types

- **number**: `12`, `-3.5`, `0.25`.
- **range**: `a..b` — uniform random per mark in [a, b]. Anywhere a number is
  accepted per-mark (radius, alpha, angle, aspect, scatter), a range is too.
  `a > b` is an error (probably a typo).
- **point**: `x,y`.
- **path**: `x,y;x,y;x,y` — points separated by `;`.
- **color**:
  `#rgb #rrggbb #rrggbbaa`, `rgb(r,g,b)` (0-255), `rgba(r,g,b,a)`,
  `hsl(h,s%,l%)`, `oklch(L,C,H)` (L 0..1, C ~0..0.4, H degrees),
  `mix(C1,C2,t)` (mix in OKLab, premultiplied by alpha), names
  `black white transparent`,
  and `$var` references.
- **colors**: comma-separated list of colours with optional weights:
  `#c33*3,#36c,$skin*2`. (Commas inside `rgb(...)` etc. are part of the
  colour.)
- **shape** (for `in=`, and `fill`/`clear`):
  `circle(cx,cy,r)`, `ellipse(cx,cy,rx,ry)` or `ellipse(cx,cy,rx,ry,rot_deg)`,
  `rect(x,y,w,h)`, `poly(x,y;x,y;...)` (straight edges),
  `blob(x,y;x,y;...)` (closed Catmull-Rom smooth curve through the points),
  `canvas` (the whole canvas).
- **bool**: `true false`.
- **name**: layer names, `[A-Za-z0-9_-]+`.

### Variables

```
set skin=#d9a07a shadow=mix($skin,#402030,0.5) r-small=1..2
```

Variables persist for the rest of the document (across commits). A variable
reference `$name` is substituted textually before value parsing, so a variable
may hold any value type.

### Common mark parameters

Ops that place marks (`dot`, `stipple`, `pointillize`, `dabs`) share:

| key      | meaning | default |
|----------|---------|---------|
| `r`      | radius (number or range) | 2 |
| `color` / `colors` | fixed colour, or weighted random choice | (required unless the op supplies colour) |
| `jitter` | per-mark colour variation: `dL,dC,dH` uniform ± (see deviations) | `0,0,0` |
| `alpha`  | per-mark opacity (number or range) | 1 |
| `mark`   | `circle`, `square`, `dab` (ellipse), `line` (short stroke) | `circle` |
| `aspect` | dab/line length ÷ width | 2.5 |
| `angle`  | degrees (number or range) or `flow` (pointillize: follow form) | 0..180 for dab/line |
| `clip`   | layer name; mark placement probability × that layer's alpha at the point | none |
| `seed`   | integer | derived |

`mark=line` draws a stroked segment of length `r*aspect*2` and width `r`.

### Ops

`layer name=N [opacity=] [blend=] [visible=] [above=L | below=L | top=true]`
: Create layer N (on top by default) or select it if it exists, updating any
  given properties. Subsequent drawing ops target the current layer. A new
  document has one layer named `paint`, selected.

`delete-layer name=N` · `rename-layer name=N to=M` · `copy-layer name=N to=M`
(M new, directly above N, with N's pixels, opacity and blend; visible and
selected)

`set k=v ...` : define variables.

`fill in=SHAPE color=C [feather=PX] [alpha=] [clip=L]`
: Solid fill. `feather` softens the edge by blurring the shape's coverage
  mask (approximate Gaussian, sigma ~ feather/2). `clip=L` multiplies the
  coverage by layer L's alpha (taken before drawing, so L may be the
  current layer: an alpha lock). `gradient`, `clear`, `blur` and `stroke`
  take `clip=` the same way; `clear clip=L` without `in=` erases where L
  is painted. There are no shape booleans: masks on hidden layers combine
  shapes (intersection via `clip=`, difference via `clear`).

`gradient in=SHAPE kind=linear|radial stops=C@t,C@t,... (from=P to=P | center=P radius=R [focus=P]) [feather=] [alpha=]`
: Gradient fill of the shape. `stops` positions are 0..1.

`clear [in=SHAPE] [feather=]` : Erase (destination-out) on the current layer;
  no `in` = clear the whole layer.

`blur r=PX [in=SHAPE]` : Blur the current layer (whole, or masked by shape).

`dot at=P [r=] color=C [alpha=] [mark=] [angle=] [aspect=]`
: One mark. Also `dot at=P;P;P ...` for several at once.

`stipple in=SHAPE (count=N | density=D) [placement=random|even] [spacing=PX] [falloff=PX] [weight=dark(L)|light(L)|alpha(L)] [gamma=G] + mark params`
: Scatter marks over the shape.
  - `density` is marks per 1000 px² of shape area.
  - `placement=even` uses Poisson-disk (blue noise) sampling with minimum
    distance `spacing` (default derived from density); `random` is uniform.
    Pointillism generally wants `even`.
  - `falloff=PX` makes acceptance probability ramp from 0 at the shape edge
    to 1 at PX inside — soft-edged clouds of dots.
  - `weight=dark(L)` accepts a candidate with probability (1 - luminance of
    layer L at that point) ^ gamma — classic tonal stippling from an
    underpainting. `light(L)` is the inverse, `alpha(L)` uses coverage.
  - `count` is the number of *candidates*; `apply` reports how many survived.

`pointillize source=L [in=SHAPE] (count= | density=) [placement=] [spacing=] [min-alpha=0.5] + mark params (colour comes from the source)`
: The central op. Places marks whose colour is sampled from layer L (usually
  a hidden underpainting) at each mark's centre, then varied by `jitter`.
  Candidates where L's alpha < `min-alpha` are skipped. `angle=flow` orients
  dabs/lines perpendicular to the source's luminance gradient (i.e. along
  contours), which reads as brushwork following form. `in` defaults to
  `canvas`.

`dabs path=PATH [smooth=true] (spacing=PX | count=N) [scatter=PX] [taper=T] + mark params`
: Marks along a path (Catmull-Rom smoothed unless `smooth=false`).
  `scatter` offsets each mark in a random direction by up to PX. `taper` 0..1 shrinks radius toward both ends. With `angle=follow`
  marks align with the path direction. `path=` may also be a shape (its
  closed outline) or `arc(cx,cy,r,a0,a1)` / `arc(cx,cy,rx,ry,a0,a1)`
  (degrees, clockwise from +x); these are exact curves, never smoothed,
  and `smooth=` with them is an error. `r`, `alpha` and `scatter` accept
  ramps `a->b` (linear from path start to end; each end a number or
  range).

`stroke path=PATH width=W color=C [smooth=true] [alpha=] [cap=round|butt] [taper=T] [clip=L]`
: A continuous anti-aliased stroke. With `taper`, it is drawn as a
  densely-spaced tapered polygon or dab chain. `path=` as for dabs;
  `width` and `alpha` accept ramps of plain numbers.

`image src=NAME [at=P] [size=W,H] [alpha=]`
: Draw an imported asset onto the current layer (for reference layers or
  collage). Default: fit to canvas.

## Implementation notes

- Crates: `tiny-skia` (rasterisation, blend modes, PNG encode/decode),
  `clap` (derive), `serde` + `serde_json`, `rand` + `rand_chacha`, `anyhow`.
  Pure Rust only — the tool must build on aarch64-darwin as well.
- Mark placement and colour choice consume randomness in a fixed order that
  does not depend on `scale`, so exports match views.
- Feathered masks: render shape coverage into a `Mask`/alpha pixmap, apply a
  3-pass box blur, use as the paint mask.
- Poisson-disk: Bridson's algorithm over the shape's bounding box with a
  background grid, rejecting samples outside the shape.
- Performance target: 200k circle marks at 1024x1024 in well under 2 s in a
  release build; replay of a 30-commit painting in a few seconds.
- Tests: DSL parsing (every value type, error messages with line numbers,
  unknown-key rejection), determinism (same doc → identical pixels, cache vs
  full replay identical), undo/redo, `--at`, scale-2 export dimension.

## Implementation notes: decisions and deviations

Where this document was silent or ambiguous, the implementation decided as
below. `easel ops` (the painter-facing reference, `src/help/*.txt`) documents
the same behaviour; keep both in sync.

### DSL

- `#` starts a comment only at the start of a token (line start or after
  whitespace) and outside parentheses, so `color=#c33` and
  `colors=#a00,#00a` are values. There are no quoted strings.
- Whitespace inside parentheses is dropped (`rgb(1, 2, 3)` = `rgb(1,2,3)`).
- `set` processes its keys left to right, so `set a=#fff b=mix($a,black,0.5)`
  works (the spec's own example needs this).
- Unknown-key suggestions use edit distance plus a small alias table
  (`radius`/`size` -> `r`, `colour` -> `color`, `opacity` -> `alpha`, ...).
- Named colours are only `black white transparent`. `oklch()` takes an
  optional 4th alpha argument; `rgba()`/`oklch()` alpha may be a percentage.
- Commit message defaults to the script's first non-empty comment line,
  else a summary of its ops (`fill, stipple x3`).
- Ranges with `a > b` are rejected with a "did you mean b..a" hint.
- `seed=` is parsed as a 64-bit integer (negative allowed, bits
  reinterpreted); fractions are errors.
- List values with an empty item (`colors=#fff,` — typically a space after
  a comma, where ` #...` then starts a comment) and bare tokens following a
  value ending in `,`/`;` get an error explaining the whitespace rule.
- Colour mixing (`mix()` and gradient interpolation) is premultiplied in
  OKLab: L, a and b are weighted by each endpoint's alpha, so fading toward
  `transparent` keeps the colour and only lowers alpha.

### Ops

- Extension: mark key `soft=PX` (number or range) widens the anti-aliased
  edge of marks; 0 (default) is the normal 1-device-px edge.
- Extension: `pointillize` also accepts `falloff`, `weight` and `gamma` (same
  code path as `stipple`). Its default placement is `even`; `stipple`
  defaults to `random`.
- `placement=even` accepts `spacing=` alone (use every Poisson point). With
  `count`/`density`, the derived spacing aims ~15% denser than the target
  and the shuffled result is truncated to exactly the target count. The
  Bridson density constant (0.63 points per spacing^2) was measured.
- Density is per 1000 px^2 of the shape's *on-canvas* area (grid-sampled);
  sampling happens in the shape's bbox intersected with the canvas.
- `weight=dark(L)` uses OKLab lightness and multiplies by L's alpha, so
  transparent areas get no marks; `light(L)` likewise; `alpha(L)` = alpha^gamma.
  `gamma=` without `weight=` is an error.
- Acceptance uses one independent roll per factor (falloff, weight, clip,
  source alpha), so `apply` can report rejections by reason.
- `pointillize` colours are the source colour made opaque; the mark's own
  alpha comes from `alpha=`. `angle=flow` uses the source's luminance
  (composited over mid-grey), blurred with sigma = max(2, r_max*aspect_max)
  canvas px; flat areas fall back to a random 0..180 angle.
- Mark geometry: `dab` is an ellipse 2*r*aspect long and 2r wide; `line` is a
  round-capped capsule 2*r*aspect long and r wide (spec); `square` has side
  2r. Coverage is analytic (signed distance), and marks whose footprint is
  at most 64 px have their coverage scaled down to their true area, so
  sub-pixel dots do not over-darken.
- `taper` profile (dabs and stroke): r * (1 - taper*(1 - sin(pi*u))), u = 0..1
  along the path. Tapered strokes are a max-combined disc chain (uniform
  alpha, always round ends); so are ramped strokes, whose discs carry
  the ramped radius and alpha (max-combined, so overlaps never darken).
- Ramps (`a->b`): a ramp with a range at either end draws one random
  number per mark and uses it at both ends, then interpolates by u; a
  ramp of plain numbers draws nothing, so it leaves the random sequence
  unchanged. Other ops reject ramps; a reversed range suggests one.
- Closed paths (shape outlines) walk back to their first point; arcs use
  points about 2 canvas px apart.
- `dabs` spacing defaults to the largest `r`; `count=1` puts one mark at the
  path midpoint. `scatter` may be a range; each mark draws its own limit
  before its offset.
- `jitter=dL,dC,dH` draws four values per mark: dL, a radial and a
  tangential chroma offset (both ±dC) and dH. The chroma change is an
  offset in the OKLab a/b plane: radial along the colour's (rotated) hue,
  plus the tangential part scaled by max(0, 1 - C/dC). Saturated colours
  therefore only vary in chroma; near-greys scatter around neutral in every
  direction. Nothing is clamped, so the mean colour equals the base colour
  (a one-sided chroma clamp and the arbitrary hue of a grey previously gave
  neutrals a colour cast).
- `placement=random` rejection-samples in the (canvas-clipped) bbox with a
  total attempt budget of count x max(64, 16 x bbox area / shape area)
  (capped), so thin and diagonal shapes still get their full count. If the
  budget runs out the report says `N of M not placed`.
- `gradient kind=` is optional: radial if `center=` is given, else linear.
  Stops are expanded to 12 sub-stops per segment so colours mix in OKLab.
- `blur r=` and `feather=` both use sigma = value/2. `blur in=` masks with
  the shape's anti-aliased (unfeathered) coverage.
- `layer`: new layers go on top; an existing layer only moves with
  `above/below/top`. `delete-layer` of the current layer selects the top
  layer (or none).
- `image` with neither `at` nor `size` fits inside the canvas keeping aspect,
  centred; `at` alone uses natural size; `size` stretches. Assets are PNG
  only (`import` rejects other formats) because tiny-skia only decodes PNG.

### Rendering, cache and determinism

- RNG is ChaCha8. The render version in the cache key is bumped whenever
  the random sequence or pixel output changes (version 2: four-draw
  jitter, premultiplied mixing; version 3: exact fixed-point blurs and
  flow fields, which moved some pixels by a level or two and turned a few
  marks in flat areas of flow passes).
- Blurs (blur, feather, the flow field) are three box passes over
  fixed-point integers (bytes x 256, luminance x 65536) with exact window
  sums and a rounded mean per pass, so a pixel's value does not depend on
  where the blurred area starts. Feathers blur only the shape's bounding
  box plus the blur's support, `blur in=` only the shape's box, and a flow
  field only the area around its candidate marks; results are identical
  to blurring the whole layer. The per-op seed is splitmix64((commit << 32) | line).
- Every per-candidate random value (attributes and acceptance rolls) is drawn
  before any raster lookup, so the random sequence is identical at every
  scale. Raster-dependent decisions (weight, clip, source alpha, source
  colour, flow) sample the layer at the scaled position and can therefore
  differ marginally between a scale-1 view and a scale-3 export.
- Cache: `cache/<hash>/` snapshots (raw premultiplied layer bytes plus
  `state.json`) keyed by a chained FNV-1a hash of (render version, size,
  background, scripts of commits 1..k). Rendering commit N resumes from the
  newest cached prefix <= N, so undo, redo and `--at` of recent states are
  cheap. The three most recently used snapshots are kept. `undo`/`redo` do
  not render. Importing over an existing asset clears the cache.
- Solo view shows the layer's raw pixels (ignoring its opacity and blend).
- `view --alpha` shows the composite's alpha; without `--solo` that is
  normally solid white, so `view` prints a note suggesting `--solo`.
- `--crop` accepts a negative origin in the plain `--crop -10,-10,50,50`
  form; `sample` moves negative points behind an implicit `--` so
  `sample DOC -5,3 --radius 2` works too.

### View

- Zoom = `--max` / crop long side. Above 1 (and without `--pixels`) the
  view re-renders: `RenderState` carries a `Viewport` (canvas origin,
  scale, device size), every op maps canvas points through it, and the
  document is replayed at scale `ceil(zoom)` for the crop plus a margin,
  then area-averaged down to the exact zoom (which may be fractional).
  Mark placement, density and Poisson sampling still use the whole
  canvas, so the random sequence is the one of a full render; marks
  outside the viewport are simply not drawn.
- The margin is estimated from the document's ops in two parts. Marks are
  placed in canvas coordinates and drawn whether or not their centre is in
  the viewport, so only raster reads need margin. The hard part is the
  largest extent of a mark that reads a layer (pointillize, `weight=`,
  `clip=`) plus three sigma of the largest flow field; cutting it changes
  whole marks anywhere in the crop (a rejected mark, a rotated dab), so it
  is never given up. The soft part is the largest blur/feather (two
  sigma); cutting it only shifts values near the crop's edges by a level
  or two, so it only fills the margin up to 160 canvas px. The region is kept under 8M device px:
  first by giving up soft margin, then by lowering the zoom (which is also
  capped at 16x), so a small crop of a document with large flow marks
  renders below 16x. The flow field's gradient spans one canvas pixel
  (`scale` device px), so high-scale angles are as stable as 1x ones.
  Measured on four 768x1024 paintings, a crop matched the same rectangle
  of an export at its scale to within one level (tiny-skia's gradient
  shader rounds differently under translation).
- A full-canvas view of a canvas smaller than `--max` is a zoom above 1,
  so it replays too instead of using the scale-1 cache.
- `--pixels` (or zoom <= 1) crops the cached scale-1 composite: zooming in
  is a whole-number nearest-neighbour factor floor(max/long side), so the
  output can be smaller than `--max`; zooming out is an area-average (box)
  filter.
- Transparent pixels (solo, transparent background) are shown over an 8 px
  grey checkerboard in output space.
- Grid lines are a 1 px dark line plus a 1 px light line, both 45% opaque.
  Labels use a 5x7 bitmap font at 2x on a dark box; the first label on each
  axis is prefixed `x`/`y`; labels that would overlap are skipped.
- `view` prints the output size, canvas region, commit, zoom and render
  scale so output pixels map back as canvas = crop origin + output px /
  zoom.
- `apply`/`amend` `-q` keep only report lines of mark ops that placed
  nothing, could not place every candidate or lay outside the canvas;
  `set` reports only the variable names (the values are in the script).

### Measured performance (release build, 1024x1024)

- 200k random circle marks, r=1..2.5: ~0.11 s (whole apply).
- 200k even (Poisson) circle marks: ~0.62 s (Bridson dominates).
- 200k random dabs with colour jitter: ~0.31 s.
- Marks are drawn in parallel over 16-row bands (each band applies every
  mark touching it in order, so pixels match a sequential draw).
- A 20-commit 768x1024 portrait (three dogfood paintings' worth of ops):
  replay from scratch 0.6 s, cached full view 0.18 s, `export --scale 2`
  1.1 s; zoomed crops 1.1 s (400x300 at 2.5x), 1.3 s (250x250 at 4x),
  2.2 s (100x75 at 10x, 64x56 at 15.6x). Pointillize and tiny-skia
  gradients dominate.
