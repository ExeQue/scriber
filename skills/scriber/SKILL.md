---
name: scriber
description: Draw and edit laser-cutter designs in Scriber, the 2D CAD program, through its MCP tools (scriber server). Use when the person asks to draw, change, check or prepare something for their laser cutter in Scriber, to follow their review comments on the canvas, or to set up machines and materials.
---

# Working in Scriber

Scriber is a 2D CAD program for laser cutters. A person sits in front of the Scriber window while you
edit the same document over MCP: every change you make shows up live on their screen, and they can
undo it. Work like a careful colleague at their desk: look first, change in small steps, show the result.

The server's own instructions list the tools and their arguments. This skill is about working well
with them.

## The drawing model

- **Units and coordinates**: millimetres. The origin is the top-left corner of the material, x grows
  right, y grows **down** (as in SVG). Angles are degrees, positive = **clockwise** on screen
  (0° = right, 90° = down).
- **Layers carry the laser operation**, as in xTool Studio. Every item sits on one layer, and the
  layer's op says what the laser does:
  - `cut`: cuts through along the line (red);
  - `score`: marks a thin line (blue);
  - `engrave`: burns the whole **area** inside closed paths, text and images (black); an open path on
    an engrave layer is scored;
  - `no_output`: notes, dimensions and helpers for people, never lasered (grey).
- To change what happens to an item, move it to another layer (`update_item` with `layer`, or `op`
  for the first layer with that op). To change a whole group of items at once, change the layer's op
  with `update_layer`.
- A layer can carry its own power/speed/passes; otherwise the material library's preset for its op
  is used. Two engrave layers with different settings give two engraving depths.
- **One shape, one op.** To engrave a face and cut the part out, add two items with the same shape:
  one on an engrave layer, one on a cut layer.
- **Compound paths**: a path can hold several contours; the inner ones are holes that stay empty when
  engraved. Build one with `combine_items` (or `boolean` difference), split it with `break_apart`.
- Guide lines (add_items kinds `xline`, `ray`, `centerline`, `centermark`) and dimensions help with
  placing things and are never lasered.

## A drawing session

1. **Look first.** Call `get_document`: material size, layers, the person's current selection and the
   items. On a big document it returns a summary; ask for the geometry of the ids you need.
2. **Build with `add_items`.** Prefer the simple kinds (`rect`, `circle`, `arc`, `polygon`, `line`,
   `polyline`, `text`) over hand-made paths with bulges; use `curve` for Bézier curves and
   `import_svg` with data for SVG path strings (`import_dxf` for a DXF file from a CAD program).
   Send related items in one call: it is one undo step for the person. For a very large batch, write the items to a JSON file and pass its path.
3. **Shape it with the CAD tools** (`fillet`, `offset`, `boolean`, `array_polar`, `trim`, …) rather
   than recomputing coordinates yourself.
4. **Lock the sizes the person cares about.** When they give a size ("40 mm wide", "holes 3 mm"),
   add a dimensional constraint (`add_dimensional_constraint`) so it stays true while other things
   change, and name it after its meaning (`set_parameter` can rename `d1` to `width`). Use expressions
   for relations (`d2 = d1 / 2`). Run `auto_constrain` on a shape first when it should keep its form
   (horizontal, vertical, parallel sides) as a size changes. A change that breaks a constraint is
   refused and names the conflict: read it, do not force your way around it.
5. **Look at the result often.** `export_png` without a path returns an image you can see (each layer in its colour, as on the canvas). Check after
   every meaningful step, not only at the end; coordinates in your head and pixels on the screen
   disagree more often than you expect (remember: y grows down).
6. **Measure, do not guess.** `measure` gives sizes, areas, distances and the laser path length.

### Keep replies small

Tools that change items reply compactly (id, name, layer, op, kind, bounding box, counts). Ask for
`verbose: true` or call `get_document` with ids only when you need the actual geometry. Use
`get_document` with `geometry: false` for an overview.

## Respect the person

- **Their selection is theirs.** Your edits leave it alone; items you add are not selected. Only
  `select`, `select_similar`, `quick_select` and `add_items` with `select: true` change it: use them
  when the person asked you to select something or to show them what you mean.
- Tools that take ids use the person's selection when the ids are empty. That is how "this one"
  and "these" reach you: read the selection from `get_document` and confirm what it holds before you
  change it.
- **Several documents.** The person may have several documents open in tabs. Tools work on the
  active tab unless you pass the optional `document` argument; keep working on the document you
  started on, and do not switch the person's active tab unless they ask.
- **Undo is shared.** `undo` takes back the last change, whoever made it, the person's included.
  Only undo your own step, right after you made it; if the person edited in between, fix your change
  with a new edit instead.
- `open` and `new_document` open a new tab and leave the person's active tab as it is (`activate`
  defaults to false; pass `activate: true` only when they want to see it). `save` writes their file
  and `close_document` refuses unsaved changes unless told to discard them: do these only when asked,
  and never discard changes without asking.
- Leave layers the person locked or hid as they are.

## Review comments

The person can pin comments on the canvas, on a spot or on shapes, to tell you what to change. You
can leave notes for them the same way (`add_annotation`). Comments are never lasered, exported or
undone.

To follow a review:

1. `list_annotations` with `status: open` shows what is waiting; keep the `revision` it returns.
2. Loop on `wait_for_annotations`, passing the revision from its last reply. It blocks until the
   person adds, replies to, resolves or reopens a comment (or times out with `timed_out: true`;
   then just call it again). Your own changes never wake it.
3. For each comment: look at the items it points at, make the change, check it with `export_png`.
4. Answer with `reply_annotation`: `resolve: true` when done, saying in one or two sentences what you
   changed. When the comment is unclear, ask in a reply and leave it open instead of guessing.
5. Go back to step 2 until the person says the review is over.

## Getting ready to laser

1. Machine, laser module and material: `list_machines`, `list_materials`, then `set_laser_setup`
   (`use_work_area: true` also sizes the canvas to the machine's work area).
2. Settings: `get_laser_setup` and `list_layers` show the power/speed/passes each layer will use.
   Give a layer its own numbers with `update_layer` (settings), or store them for the material with
   `set_material_setting`. Engrave line spacing is `line_interval` in mm; people think in lines per
   cm, so `line_interval = 10 / lines per cm`. `update_layer` takes it as `lines_per_cm` directly.
3. Kerf: for parts that must fit, `offset` the outline by half the kerf and cut the copy.
4. Run `preflight_check` and fix what it reports (open shapes set to cut, double lines, shapes
   outside the material, details smaller than the beam, ops without settings).
5. Look at `export_png` with `preview: true`: it shows what the laser makes (burned areas, cut-out
   pieces with their holes). Show it to the person before they laser.
6. `export_svg` (or `export_dxf` for CAD programs) writes a file for other laser software, if they want one. Scriber does not run the
   laser; the person sends the job from their laser software.

## Materials

- `search_xtool_materials` searches xTool Studio's local cache (only materials the person opened
  there); `import_xtool_materials` copies the ones they pick into their Scriber library for one of
  their machines. `import_xtool_project` takes the settings of an `.xs` project; `import_xs` opens
  the whole project (shapes, text, images, layers with their settings, groups) as a new document.
- `export_library` and `import_library` write and merge `.scrlib` files (or library zips, `.zip`)
  to share machines and materials with other Scriber users.
- **Library sources**: people can subscribe to machine and material libraries published in public
  GitHub repositories with `subscribe_library` (`owner/repo`, `owner/repo@v1.2` or a link; a `track`
  follows a branch, a tag, the latest release or one commit). Their items are read-only; saving one
  with `save_machine`/`save_material` under the same id makes the person's own copy, which wins.
  The editor syncs sources at start at most once a day; `sync_library_sources` syncs now and says
  what changed. `list_library_sources` shows them, `set_library_source` changes the track or turns
  one off, `unsubscribe_library` removes one (ask first).
- To publish their own machines and materials, `export_library_repository` writes them into a
  folder (new, or an existing git repository updated in place); the person commits and pushes.
- Add or correct a material with `save_material` and `set_material_setting`. Ask before changing
  numbers the person already tuned.
- Remove with `delete_material`, `delete_material_setting` (one profile; no name = the default one)
  and `delete_machine` (refused while materials have settings for it, unless `force`). Deleting
  cannot be undone: ask the person first, then say what was removed. Source items are read-only. A
  deleted built-in machine comes back with `restore_machine`.

## Recipes

Coordinates below assume a material of at least 100 × 100 mm; adjust to `get_document`.

**Coaster with an engraved face**: the same circle twice, on two layers.

```json
{"items": [
  {"shape": {"kind": "circle", "cx": 50, "cy": 50, "r": 45}, "op": "engrave", "name": "face"},
  {"shape": {"kind": "circle", "cx": 50, "cy": 50, "r": 45}, "op": "cut", "name": "outline"}
]}
```

To engrave only a ring, make the face a compound path: add a smaller circle and `combine_items` it
with the engraved one (the inner circle becomes a hole).

**Text to engrave**: kind `text`, (x, y) is the start of the baseline, `size` is the font size in mm.
Text goes on an engrave layer by default and is lasered as glyph outlines. `list_fonts` lists the
installed families (`sans-serif`, `serif`, `monospace` always work).

```json
{"items": [{"shape": {"kind": "text", "x": 20, "y": 55, "size": 10, "text": "Hello", "font": "serif"}}]}
```

The reply's bounding box tells you where the text really landed; centre it with `transform_items`
(dx/dy) or `align`.

**Ring of holes**: one hole, then `array_polar` round the centre.

```json
{"items": [{"shape": {"kind": "circle", "cx": 50, "cy": 20, "r": 1.5}, "op": "cut", "name": "hole"}]}
```

then `array_polar` with the hole's id, `count: 6`, `centre: [50, 50]` (count includes the original;
`angle` defaults to 360, evenly round). For a grid use `array_rectangular`.

**A plate of fixed width**: a `rect` on a cut layer, `auto_constrain` on it so it stays a rectangle,
then `add_dimensional_constraint` with type `linear` on its top edge (`{kind: segment, item, segment: 0}`
for a rectangle without corner radius) and name `width`. Later `set_parameter` with name `width`
resizes it, and everything constrained to it follows.
