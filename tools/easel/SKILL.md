---
name: easel
description: Paint raster images (pointillist portraits, illustrations) mark by mark with the `easel` CLI, seeing the work through rendered PNG views. Use when asked to paint, draw or illustrate something as an image made by Claude itself rather than a diffusion model. Image-heavy; run it in a fork or subagent.
---

# Painting with easel

`easel` is a painting tool built for you. A painting is a directory
(`name.easel/`) holding a log of scripts. You `apply` a batch of paint
operations, `view` the result as a PNG, **Read** that PNG to see it, and
either continue or `undo`. Everything is deterministic and
resolution-independent: `export --scale 3` re-renders every mark crisply.

## Before you start

- **Context.** Every view you Read costs context. When the main conversation
  asks for a painting, do the painting in a subagent or fork, and return only
  the final PNG path(s), the document path and a short description. If you
  are already a subagent, paint directly.
- **Learn the DSL from the tool**, not from memory: run `easel ops` once
  (the full reference, ~300 lines) or `easel ops OPNAME` / `easel ops
  workflow|syntax|values|marks` for one part. The reference is authoritative;
  this skill only covers how to paint well with it.
- Work in the directory the user named, or the scratchpad if they named none.
  Keep scripts in files (`01-under.txt`, `02-pass1.txt`, ...) or heredocs.

## The loop

```sh
easel new face.easel --size 768x1024 --bg "#101018"
easel apply face.easel -m "block in" <<'EOF'
layer name=under visible=false
fill in=blob(384,180;520,300;540,520;460,760;384,820;300,760;230,520;250,300) color=#c9a58a feather=18
EOF
easel view face.easel --solo under       # then Read face.easel/view.png
easel apply face.easel -m "first pass" pass1.txt
easel view face.easel                     # Read it
easel view face.easel --crop 280,380,220,120   # zoom on the eyes, with grid
easel undo face.easel                     # if the last step was a mistake
easel export face.easel -o face.png --scale 2
```

- **View after every apply.** You are blind between renders. Read the whole
  canvas after each pass, and a zoomed crop (grid on by default) whenever you
  place something that needs precision — eyes, mouth, edges of a silhouette.
  A crop is re-rendered at its zoom (up to 16x), so marks are as sharp as in
  an export: the smaller the crop, the closer the look. It takes a second or
  two. `--max` sets the output size; `--pixels` shows the actual 1x pixels.
- **Use the grid to place things.** Grid labels are canvas coordinates. When
  something is in the wrong place, read its coordinates off a crop and fix
  the numbers rather than guessing.
- **Use text feedback too.** `apply` reports marks placed and rejected per
  line; `easel info` gives layers, coverage and bounding boxes; `easel
  sample X,Y` gives exact colours. These cost far less context than an image.
- **Undo freely.** One apply is one undo step, so keep applies to a coherent
  pass (5–20 lines). `--dry-run` checks a script without committing;
  `view --at N` shows history. An apply (or amend) after an undo
  discards what was undone. To change an old pass without redoing everything above it:
  `easel show DOC N > f.txt`, edit, `easel amend DOC N f.txt`.
- **Keep the output short.** Long generated scripts make long reports; `apply
  -q` prints only mark ops that placed nothing or not everything.

## How to paint a portrait

Work coarse to fine, as a painter would.

1. **Composition first.** Decide canvas size, where the head sits, the light
   direction and a small palette (`set skin=... shadow=... light=...`).
   Limited palettes read as intentional; 4–7 colours plus jitter is plenty.
   Plan layers: background and figure on separate visible layers, figure on
   top (rename the default `paint` layer rather than leaving it empty).
   Resist perfect symmetry; a slightly turned or asymmetric face reads as
   alive, a mirrored one as a mask.
2. **Hidden underpainting.** On `layer name=under visible=false`, block in
   big shapes with `fill` and `gradient`, soft edges via `feather=`: the
   background, the silhouette, the large planes of light and shadow, the eye
   sockets, the hair mass. Check it with `view --solo under`. This layer is a
   colour-and-value map, not the painting; get the values right here, since
   everything after samples from it. A later fill covers what is under it,
   so repainting one shape also covers the part of any earlier shape it
   overlaps; repaint those too. Repaint a region with feathered, oversized
   shapes and keep the background values unchanged, or you will get seams.
3. **Pointillize it.** On a visible layer, `pointillize source=under` in
   several passes, from large to small marks:
   - a covering pass with big marks (`r=4..7`, `mark=dab`) so the ground
     doesn't show through where it shouldn't. `angle=flow` suits the figure;
     on flat areas it gives random angles, so give the background a fixed
     angle range (`angle=55..70`) on its own layer. A copy of the
     underpainting as a blurred base (`copy-layer name=under to=base`, then
     `layer name=base below=...` and `blur`) fills gaps between marks;
   - a medium pass with more jitter. Keep it sparser, lower-alpha or
     restricted with `in=`: a dense pass of small marks over everything
     smooths the big marks away and the painting turns airbrushed;
   - fine passes (`r=1..2`, higher density) restricted with `in=` to the
     focal areas — eyes, mouth, the lit edge of the face.
   `jitter=dL,dC,dH` in OKLCH: small L (0.03–0.06), moderate H (10–30°) gives
   vibration without mud. Complementary accent dots at low alpha (a few cool
   dots in warm shadows) are what makes pointillism shimmer.
4. **Accents.** `dot`, `dabs` along paths, `stroke` for crisp lines,
   `stipple` with `weight=dark(under)` for tonal texture. `weight=light(...)`
   highlights sprinkle specks over every mid-tone unless you give them a
   tight `in=`, `gamma=` 5 or more and colours only a little lighter than
   the skin.
   - `clip=L` keeps marks, fills, gradients and blurs on pixels already
     painted on L (a shading gradient stays on the figure instead of
     darkening a rectangle of background). Combine shapes on hidden mask
     layers: intersection via `clip=`, subtraction via `clear in=`.
   - Rings and arcs: `path=circle(...)` or `path=arc(cx,cy,r,a0,a1)` for
     dabs and stroke. Fades and trails: ramps along the path, e.g.
     `alpha=0.9->0.1`, `r=4->1`, `scatter=2->30`.
   - A `stroke` is a clean vector line and can look pasted onto dots; for
     a drawn line use `dabs ... angle=follow` with spacing below r and a
     little scatter. For a cloud or particle seam rather than a string of
     pearls, use a scatter several times r.
   - Marks cannot be thinned out afterwards (a feathered `clear` fades
     them all). Build dissolving edges while placing: `falloff=`, or
     `weight=alpha(mask)` from a hidden mask layer.
5. **Judge at two distances.** The full view says whether the image reads;
   crops say whether the marks are good. Both matter.

## Taste notes

- Edges carry the drawing. Hard, precise edges (small marks, crisp `in=`
  bounds) on the focal features; everything else can dissolve.
- Faces are read from eyes first. Spend your detail budget there. Dark
  features (lid lines, nostrils, the mouth line) read well carved out as
  gaps with a feathered `clear`. Catchlights need r=1.3 or more to survive
  next to a dark pupil.
- Value structure beats colour: if the underpainting's lights and darks are
  wrong, no amount of dots saves it. Check `view --solo under` before
  pointillizing, and fix it with undo rather than painting over.
- Leave some ground showing if it helps: gaps between dots are part of
  pointillism. Cover fully only where you want solidity.
- A painting doesn't have to be a person. Machines, patterns, light, the
  impossible — use what fits the brief.

## Finishing

Export with `--scale 2` (or 3 for print), Read the export once to confirm,
and report the export path, the `.easel` document path (so the painting can
be continued or re-exported later) and a sentence or two about the choices
you made.
