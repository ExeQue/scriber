# xTool Studio `.xs` project format

Reverse-engineered from three real projects written by xTool Studio 1.9.11 (Linux), device M2 (`JS002`), module `laser_blue_10W`.
Not an official spec. Everything marked **(guess)** is inference, **(unseen)** means no sample contained it.
Samples were small: the first three had only `PATH` and `LINE`. A fourth sample (`unknowns`, same app version) added the native primitives `RECT`, `CIRCLE`, `STAR`, `REGULAR_POLYGON` (section 2b), a freehand `PATH` with `Q`/`C` segments, and the observation that elements bound to a processing type keep default fill/stroke flags.
**It did NOT contain** text, bitmaps, real groups, rotated or skewed elements, patches (all defaults, no material), or a `BITMAP_ENGRAVING` profile.
A fifth sample (`unknowns-2`, same version) contains only **text and images** (2 `TEXT`, 7 `BITMAP`, one bitmap profile, 5 image-mode patches; section 2c, 2d). It has **no** groups, rotated elements, custom power/speed or library material: those stay **(unseen)**.
Test every claim against the real app before relying on it; the safest first step is "write a file, open it in xTool Studio, re-save, diff".

## 1. Archive layout

A zip (deflate, with directory entries; entry order in the samples: `meta/`, `project.json`, `profiles.json`, `canvases/...`, `devices/...`, `resources/...`, `vectors/...`, `.format` last). Format "v2" ("xcs-workspace-v2", schemaVersion 2.1.0, written as `migratedFrom: "1.0"`).

| Path | Role | Required? |
|---|---|---|
| `.format` | the 2 bytes `v2`, no newline | yes (guess: it is the format sniff) |
| `meta/persistence-meta.json` | `{"schemaVersion":"2.0.0","protocol":"xcs-workspace-v2"}` | yes (guess) |
| `project.json` | project id, name, active canvas/device, app version, lists of canvas and device ids, cover path | yes |
| `profiles.json` | `{"profiles": {<profileId>: {id, processingType, values}}}`: per processing type the base values | yes |
| `devices/device-<code>-<n>.json` | one per device (`device-JS002-1.json`): per canvas, the material, process data, patches (overrides) and bindings (which element uses which) | yes |
| `canvases/<canvasId>.json` | canvas metadata: layers (colour), groups, chunk layout | yes |
| `canvases/<canvasId>/displays-<k>.json` | the elements ("displays"), chunk `k` (`chunkLayout.chunkIndexes`) | yes |
| `vectors/svg/index.json`, `vectors/svg/data-<k>.json` | content-addressed store of long path data (only when a path references it) | optional |
| `resources/project-cover.png` + `.meta.json` | thumbnail shown in the project list | optional (guess; `project.json.cover` points at it) |

Images (bitmaps) live in `resources/<sha256 of the file bytes>.<ext>` (verified: `.png`, the original file stored as imported, RGBA 8-bit) plus a sidecar `<same name>.meta.json` `{"ref":"resources/<name>","metadata":{"kind":"image","source":{"type":"workspace","value":"<name>"}}}` (no `mimeType` here, unlike the cover). Duplicated images share one resource file. See 2c.

### How ids link

```
project.json
  activeCanvasId ─────────────► canvases/<canvasId>.json   (id, layerData, groupData)
  modules.canvases[] ─────────►        └ canvases/<canvasId>/displays-0.json  (displays[].id)
  activeDeviceId "JS002-1" ───► devices/device-JS002-1.json  (id "JS002-1")
  modules.devices[]

device.processing[<canvasId>].modes.LASER_PLANE
   .profileRefs[]   ──► profiles.json profiles[<profileId>]
   .patches{<patchId>: {profileId, overrides}}      power/speed/... for that profile
   .bindings[] {baseProfileId, patchIds[], displayIds[]}   display ─► processing
   .ignoredDisplayIds[]                              displays that are not processed
display.vectorRef.vectorHash ──► vectors/svg/data-k.json entries[hash]  (path string)
display.layerTag  ──► canvas.layerData key
display.groupTags[] ─► canvas.groupData key
```

## 2. Canvas and elements

### `canvases/<id>.json`
```
{ "id": "<uuid>", "title": "{panel}1", "hidden": false,
  "layerData": { "#fe0002": {"name":"#FE0002","order":1,"visible":true}, ... },
  "groupData": { "g-<uuid>": {"groupName":"","groupTag":"g-<uuid>","visible":true,"enableTransform":true,"zOrder":1,"rotation":0}, ... },
  "batchData": {"entryType":"BATCH_TEMPLATE","bindings":[]},
  "extendInfo": {"version":"2.16.5","minCanvasVersion":"0.0.0","displayProcessConfigMap":{},"rulerPluginData":{"rulerGuide":[]},"type":"2d"},
  "chunkLayout": {"displayCount":N,"chunkCount":1,"chunkIndexes":[0]} }
```
- `layerData` is keyed by lowercase `#rrggbb`; xTool Studio's "layers" are colours: `name` is the upper-case hex (a user rename is possible, **guess**), `order` is 1-based display order, `visible` the eye toggle. `batchData.entryType` was `BATCH_TEMPLATE` in one sample and `VARIABLE_DESIGN` in another (meaning unknown, likely irrelevant).
- `groupData`: every group the elements belong to, keyed by tag. A group has no parent or member list; membership and nesting live only in each display's `groupTags` (section 2e). Group `rotation` was seen as `-12.566...` (= -4 pi; unit unclear, radians vs degrees) in an older sample and as `0` in `unknowns-3`.

### `displays-<k>.json`
`{"canvasId","chunkIndex","displays":[ ... ]}`. Each display has the common fields:

`id` (uuid), `name` (null), `type`, `x`,`y` (world bbox top-left, see below), `angle`, `scale{x,y}`, `skew{x,y}`, `pivot{x,y}`, `localSkew{x,y}`, `offsetX`,`offsetY`, `lockRatio`, `isClosePath`, `zOrder` (int, higher = on top; group members share), `groupTags[]` (or a lone `groupTag`: a single uuid in the old-style LINE/PATH of one sample), `layerTag`/`layerColor` (`#rrggbb`), `visible`, `visibleState`, `lockState`, `originColor`, `enableTransform`, `resourceOrigin` (""), `customData` (`tabBreaks{}`, `startPoint{}`, optionally `from.templateId`), `rootComponentId` (""), `minCanvasVersion` ("0.0.0"), `alpha` (1), `fill{paintType:"color",visible,color,alpha}`, `stroke{paintType,visible,color,alpha,width,cap,join,miterLimit,alignment}`, `effects[]`, `transform[a,b,c,d,e,f]`, `width`, `height`, `isFill`, `lineColor` (int, rgb), `fillColor` ("#rrggbb").

Types seen: `PATH`, `LINE`, and the native primitives `RECT`, `CIRCLE`, `STAR`, `REGULAR_POLYGON` (section 2b). Also seen: `TEXT`, `BITMAP` (section 2c). Still **(unseen)**: QR, groups-as-elements. A rectangle or circle drawn in the app IS stored natively (not as `PATH`). Safest export is still `PATH` for everything (always read; native primitives carry no geometry, so a reader needs the type-specific rules below).

- `PATH` extra fields: `points: []`, `fillRule` (`"nonzero"` / `"evenodd"`), `graphicX`,`graphicY` (equal to `offsetX`/`offsetY` in samples), `isCompoundPath` (multi-contour), `pathCornerData: null`, `sourceId` (optional; shared by pieces that came from one split path), and exactly one of
  - `dPath`: inline SVG path string (absolute coordinates, `M L H V C Z` seen; commas not needed), or
  - `vectorRef: {"vectorHash": "<sha256 hex>", "bucketType": "svg", "originalField": "dPath"}`: the long path string moved to `vectors/svg/data-0.json`. Seen only on a 63 KB path; short ones stay inline (threshold unknown, **guess** a size limit). Prefer inline for export; if a file is accepted with arbitrarily long inline `dPath`, no vector store is needed.
- `LINE` extra field: `endPoint {x,y}`: a straight segment from local (0,0) to `endPoint` (before `transform`), `height` 0.001.

### Coordinates and transform (verified numerically on the samples)
- Unit mm, canvas coordinates, y grows down **(guess from layout; the samples never show the machine origin)**. The canvas is not tied to the material: positions like x 400, y 280 occur. Origin versus the machine bed is not in these files.
- `transform = [a,b,c,d,e,f]` is an SVG-style affine matrix applied to the raw geometry: `world = [a c; b d] * (dPath coordinates) + (e,f)`. Check: a path with `dPath` min x = 171.07, `scale.x` .898, `e` 17.41 has `x` = 171.07 (= 171.07*.898 + 17.41).
- `x`,`y` = top-left of the world bounding box (after `transform`), `width`,`height` = world bbox size (scaled). `offsetX`/`offsetY` = `e`/`f`. `scale` = (`a`,`d`) when unrotated. So for an unrotated, unscaled path whose dPath is already in world coordinates: `transform=[1,0,0,1,0,0]`, `x,y` = bbox min, `width,height` = bbox size. (One sample has dPath in world coordinates with `f` = -3 and `y` = 3 less than the dPath min y, i.e. the bbox then includes the transform; keep `x,y` = bbox of `transform * dPath`.)
- Rotation (verified on 2 rotated `RECT`s, a `CIRCLE` and a triangle in `unknowns-3`, see 2e): `angle` is in **degrees**, positive = clockwise on screen, and `transform = [cos(angle), sin(angle), -sin(angle), cos(angle), e, f]`. The rotation is about the element's own top-left `(e,f)`; `x,y` and `offsetX/Y` stay `= e,f` (NOT the rotated bbox), `width`/`height` stay the unrotated size, `pivot` stays 0, `scale` 1. Also verified on a rotated `CIRCLE` (with the 0.01 scale) and a `REGULAR_POLYGON`; `PATH`, `STAR`, `TEXT`, `BITMAP` rotation is **guess** (not seen rotated). Safe export: bake rotation into the path data and write angle 0.
- Stroke width in the samples is 0.33 to 1.64 (display width, doesn't affect cutting). `lineColor` is an int of the layer colour (e.g. 0x9928FF, 16421416) and `fillColor` a hex; they seem to be UI defaults, **not** the layer colour: the real colour lives in `stroke.color` (int, e.g. 16711680 = red) and `originColor` ("#ff0000"), `layerTag`/`layerColor`. In two samples `stroke.color` = 0 and `originColor` "#000000" with `layerTag` `#00befe` (imported path whose colour was reassigned); so which of the four is authoritative is **unknown**; write them all consistently. The `unknowns` sample shows the same split on all 7 elements: `layerTag`/`layerColor` `#00befe`, `stroke.color` 0, `originColor` `#000000`, and `lineColor` 16421416 / `fillColor` `#f9932b` identical on every type (UI defaults, unrelated to the layer). Strong evidence that `layerTag` (matching `layerData`) is what determines the layer and `stroke.color`/`originColor` are not; none of them is used for processing.

### 2b. Element types (all fields are on top of the common set above)

Native primitives have **no** `dPath`, `points`, `fillRule`, `graphicX/Y`, `isCompoundPath`, `pathCornerData`. Their outline is implied by type + `width`/`height` + type fields; an importer must generate it. `width`/`height` are the world size (after `transform`).

| `type` | extra fields | geometry (observed) |
|---|---|---|
| `PATH` | `dPath`, `points:[]`, `fillRule`, `graphicX/Y`, `isCompoundPath`, `pathCornerData` | SVG path string, here with `M`, `C`, `Q` (a freehand/pen path, open: `isClosePath:false`, stroke only, `fillRule:"nonzero"`). `transform` identity, `x,y` = path bbox min (exact bbox incl. curve extents), `offsetX/Y`=0. Path-string coordinates may be comma-separated pairs `x,y x,y` and long decimals. |
| `LINE` | `endPoint{x,y}` | segment (0,0) to `endPoint`, `isClosePath:false`, `lockRatio:false`, stroke only. `offsetX/Y` = `e,f` = start point = `x,y` (here dx,dy positive; a line going up/left: unseen). |
| `RECT` | `radius`, `maxRadius`, `primitiveCornerData` | rectangle of `width` x `height` with its top-left at `(e,f)` = `x,y` = `offsetX/Y`. `radius` = corner radius in mm (0 = square; `primitiveCornerData:null` then). `maxRadius` = min(width,height)/2 (verified: 24.297 = 48.594/2). Rounded: `primitiveCornerData` is a **JSON string** `{"nodes":[{"id":"<uuid>","key":"top-left"},{..."top-right"},{..."bottom-right"},{..."bottom-left"}],"defaultCorner":{"desiredRadius":<r>,"style":"round"}}` (per-corner radii are presumably extra node fields, **guess**). |
| `CIRCLE` | none | ellipse inscribed in the `width` x `height` box with top-left `(e,f)` = `x,y`; one type for circle and ellipse (here 64.4 x 52.9). **Odd (still unexplained; `unknowns-3` has it again, rotated, see 2e):** `scale` and `transform a,d` were 0.01 (not 1) while `width`/`height` held the real size: a resize-artefact, **guess**: `width/height` win and the unit geometry is not 1 x 1. When writing, try `scale 1,1`, `transform [1,0,0,1,x,y]`; if the app draws it wrong, fall back to `PATH`. |
| `STAR` | `count` (points, 5), `starInnerScale` (inner/outer radius, 0.382), `oWidth`, `oHeight`, `primitiveCornerData` | regular star. `oWidth/oHeight` = the nominal (untrimmed) box, `offsetX/Y` = `e,f` = top-left of that box; `x,y,width,height` = the TIGHT bbox of the actual outline, so `x - offsetX` = 1.73 = (70.72 - 67.26)/2 (horizontally centred) and `y == offsetY` (a point touches the top). |
| `REGULAR_POLYGON` | `count` (sides, 3), `oWidth`, `oHeight`, `primitiveCornerData` | same convention as `STAR` (nominal box `o*` + tight bbox `x,y,width,height`; triangle: `x - offsetX` = 4.39 = (65.59 - 56.80)/2). Orientation / vertex 0 position: **unknown**, presumably apex up. |
| `TEXT`, `BITMAP` | see 2c | - |
| `QR`, group | **(unseen)** | - |

All primitives: `transform=[1,0,0,1,offsetX,offsetY]` (except the CIRCLE oddity), `isClosePath:true`, `lockRatio:true`, `fill.visible:true`, `stroke.visible:false`, `isFill:false`.

Minimal native examples (common fields as in section 4, only the differing ones shown):
```json
{"type":"RECT","x":10,"y":20,"offsetX":10,"offsetY":20,"width":50,"height":30,"transform":[1,0,0,1,10,20],
 "radius":0,"maxRadius":15,"primitiveCornerData":null}
{"type":"CIRCLE","x":10,"y":20,"offsetX":10,"offsetY":20,"width":40,"height":40,"transform":[1,0,0,1,10,20]}
{"type":"STAR","x":..,"y":..,"offsetX":10,"offsetY":20,"oWidth":40,"oHeight":40,"width":..,"height":..,
 "count":5,"starInnerScale":0.382,"primitiveCornerData":null,"transform":[1,0,0,1,10,20]}
```
(Untested as written; STAR/POLYGON `x,y,width,height` need the tight bbox, which a writer would have to compute.)

### 2c. TEXT and BITMAP (from `unknowns-2`)

**`TEXT`** (common fields plus): `text` (string; multi-line **unseen**), `resolution:1`, `fillRule:"nonzero"`, `isClosePath:true`, `isFill:false`, `fill.visible:true`, `stroke.visible:false`, `customData.from.officialFontId` (int, only for one of the two fonts; omit), and
- `style`: `{fontSize, fontFamily, fontSubfamily:"Regular", fontSource:"build-in", letterSpacing:0, leading:0, align:"center", curveX:10, curveY:0, isUppercase:false, isWeld:false, direction:"auto", writingMode:"horizontal-tb", textOrientation:"mixed"}`. Both fonts were xTool built-ins (`fontSource:"build-in"`); a system/custom font value is **unseen**. `letterSpacing`/`leading` = 0 is the only value seen (unit **unknown**); `align` was `center` for single lines (other values **unseen**, probably left/right).
- **Size:** `fontSize` is in points at the displayed size and equals `72 * scale.x` exactly (223.16 = 72 * 3.0995; 112.98 = 72 * 1.5692). The stored glyph geometry is for a 72 pt (= 25.4 mm) em, so `world = transform * glyph coordinates` and the real em height in mm is `fontSize * 25.4 / 72` **(verified numerically: capHeight 18.288 * 3.0995 = 56.7 mm)**. `scale` is uniform (`lockRatio:true`); resizing text changes `scale` and `fontSize` together.
- `fontData`: `fontInfo{unitsPerEm:1000, lineHeight, ascent, descent (negative), capHeight, xHeight, lineGap}` in mm at the 25.4 mm em (lineHeight about 1.22-1.25 em); `glyphData{<char>:{dPath, advanceWidth, advanceHeight, leftBearing, topBearing, bbox{minX,minY,maxX,maxY}}}` (one entry per distinct char, **outlines with y negative upward**, baseline y = 0, `M L Q Z` only); `glyphDataByGid{<json string>: ...}`; `layout{paragraphDirection, lines[{index, baseDirection, clusters[{chars, logicalIndex, span, sourceSpan, direction, bidiLevel, glyphs[{gid,dx,dy,advanceWidth,advanceHeight}], advanceWidth, advanceHeight, graphemeCount, fontFamily, fontSubfamily, fontSource}]}]}` (shaping result).
- **`charJSONs`**: an array with **one ready-made `PATH` display per character**, i.e. the glyph outlines ARE stored (each: `type:"PATH"`, `dPath` of that glyph at the 72 pt scale, same `scale`/`transform` scale as the text, own `offsetX/Y` = glyph origin on the baseline in world mm, `fill.visible:true`, `zOrder:0`, `isClosePath:true`, `customData:{}`). So an importer can ignore fonts and read `charJSONs[*]` as ordinary paths (counter-holes such as in `e` are inside one `dPath`, `nonzero` direction-based, **guess**).
- Geometry of the text element itself: `transform=[s,0,0,s,offsetX,offsetY]`, `offsetX/Y` = (text origin: left edge of the first advance, baseline); `x,y,width,height` = tight bbox of the outlines (so `x != offsetX` by the first left bearing; `y < offsetY`: y is the top of the ink, baseline at `offsetY`). `width/height` are world mm.
- Both texts were bound to `VECTOR_ENGRAVING` (score, outline) in the base binding, **not** to the fill type; fill vs line is chosen in the binding, as for other shapes.
- Export guess: write text as outlines (`PATH`) as now. Native `TEXT` would need a full `fontData`/`charJSONs` set computed from a font xTool knows (**do not attempt**).

**`BITMAP`** (common fields plus; `isClosePath:false`, `isFill:true`, `lockRatio:true`, `angle:0`):
- `resourcePath:"resources/<sha256>.png"`, `format:"png"`, `originWidth`/`originHeight` (source pixels, 1280 x 640), `currentUrl:""`, `opacity:1`, `mask:null`, `modifyData:{}`, `warpLocalOffset:null`, `filterList:[]`, `filterList_V2:[]`, `originAutoAdjust:null`, `autoStrength:0`.
- **Size:** `scale = (s,s)`, `transform=[s,0,0,s,offsetX,offsetY]`, `x,y = offsetX/Y` (top-left), `width = originWidth * s`, `height = originHeight * s` (verified: 1280 * 0.114754 = 146.88), and `dpi{dpiX,dpiY} = 25.4 / s` (221.34 here). So the image is a unit-pixel raster scaled by `s` (mm per pixel); no separate crop was seen (`mask:null`; crop **unseen**).
- **Image adjustments**, all defaults, identical on all 7 copies: `grayValue:[0,255]` (levels range), `sharpness:50`, `brightness:0`, `contrast:0`, `saturation:0`, `temperature:0`, `tone:0`, `colorInverted:false`, `colorInvertedFillTransparent:false`, `colorInvertedTransparentColor:"black"`, `isGray:false`. Value ranges **unknown** (brightness/contrast likely -100..100, **guess**).
- `filterAttrsMap` (the app's stored per-filter parameters; filters themselves not applied here, `filterList` empty): `emboss{strength:5}`, `halftone{radius:4,angle:45}`, `binary{threshold:128}`, `sketch{strength:2}`, `dot{angle:45,scale:14}`.
- `sourceId`: id of the display this one was duplicated from (omit on a first image).
- The image mode is **not** on the element: it is `bitmapMode` in the bound profile/patch (2d).
- **Bytes:** the original PNG is stored untouched; the app does not store a dithered result. Any greyscale/RGBA PNG should work (**guess**; only 8-bit RGBA seen).

### 2d. Image engraving and its modes

`profiles.json` then has a fourth profile (processingType `BITMAP_ENGRAVING`; created because the project has a bitmap, also listed in `profileRefs`), values: `power:1, speed:80, repeat:1, laser:"laser_blue_10W", density:100, dpiX:254, dpiY:254, bitmapScanMode:"zMode", bitmapMode:"grayscale", processHead:"LASER", powerCutoffType:"FOLLOW_ACTUALLY_POWER", powerCutoffValue:3, airPump:15`. (No `kerf*`, `scanAngle`, `ramp*`, `fill_type`.)

`bitmapMode` values seen (this is the "image mode" dropdown; there is no "dither" entry, each dither algorithm is its own mode): `grayscale` (profile default, element with no patch), `Jarvis`, `Bayer`, `Stucki`, `Atkinson`, `Sierra`. Exact case as written. No extra per-mode parameters exist: every patch has the same key set as the profile and only `bitmapMode` differs (threshold-like parameters are in the element's `filterAttrsMap`, not in the profile). Other modes the app may offer (e.g. Floyd-Steinberg, Burkes, black/white threshold) are **unseen**; do not guess names.

Binding pattern: one base binding on the bitmap profile for the grayscale image (no `patchIds`), and one binding per distinct patch (`patchIds:[<patch>]`, `displayIds` = the images using it; two Bayer images share one binding+patch). The text elements sat in the VECTOR_ENGRAVING base binding with its two empty shadow bindings; the bitmap bindings have **no** shadows. Patches here are `source:"custom"` with the full value set in `overrides` (mode change only, power 1/speed 80 are the untouched defaults). Image scan resolution is `density` (lines/cm) with `dpiX/dpiY` = round(density * 2.54), as for fill.

**Single `groupTag`:** every element in `unknowns` has `groupTags:[]` plus a lone `groupTag:"<uuid>"` (unique per element, no matching `groupData` entry, `groupData` empty). It is a per-element id artefact, not a group. Real groups: see 2e.

### 2e. Rotation and groups (from `unknowns-3`)
The sample holds 5 `RECT`s (2 rotated), 1 `CIRCLE` (-45), 1 `REGULAR_POLYGON` (triangle, 40.41203), 2 groups, one custom patch and **no** library material. Cases:
- `angle -90`, `transform [0,-1,1,0,e,f]`, w 71.4 h 50.1: corner `(w,0)` maps to `(e, f-w)`, `(0,h)` to `(e+h, f)`: the rect extends up and to the right of `(e,f)`.
- `angle -25.71187`, `transform [0.900987,-0.433846,0.433846,0.900987,e,f]`: cos(25.71187 deg) = 0.900987 and sin = 0.433846 match to 6 digits. Negative angle = counter-clockwise on screen (y down); positive = clockwise, the same sign as Scriber's angle.
- In both, `x == offsetX == e` and `y == offsetY == f` exactly, `pivot` `{0,0}`, `scale` `{1,1}`, `skew` 0, `width`/`height` unchanged by the rotation (so `x,y,width,height` is NOT the world bbox once rotated). World corners: `P = [a c; b d] * (u,v) + (e,f)` with `(u,v)` in the unrotated box `[0,w] x [0,h]`.
- **`CIRCLE`, `angle -45`, `scale {0.01,0.01}`:** `transform = [0.007071,-0.007071,0.007071,0.007071,e,f]` = `scale * [cos,sin,-sin,cos]` exactly (0.01 * 0.707107). `x,y == offsetX/Y == e,f`, `width 32.14` / `height 44.19` = the unrotated ellipse box, rotation about its top-left like the rect. So the 0.01 is a real uniform factor in `scale` and `a..d` (the unit geometry is probably 100 x the box **(guess)**), and it composes with the rotation as `transform = scale * R`; the cause of 0.01 itself (vs 1 on RECT) is still unexplained.
- **`REGULAR_POLYGON` (triangle), `angle 40.41203`:** `transform = [cos,sin,-sin,cos,e,f]` (0.761402, 0.64828), `scale` 1, `pivot` 0, `width/height` 41.08 x 37.16 = the unrotated tight bbox, `oWidth/oHeight` 47.43 x 49.55 = the nominal box. `offsetX/Y = e,f` = top-left of the nominal box; `x,y` = `(e,f) + R * ((oWidth - width)/2, 0)` (verified: (3.1773,0) rotates to (2.419,2.060) = `x-e`, `y-f`). So even for the polygon `x,y` is the rotated position of the tight box's top-left in the element's frame, not the world bbox; the tight box is horizontally centred in the nominal box and top-aligned (dy 0; apex up, **guess** for other counts).
- Rotation rule summary: always `transform = scale * [cos(angle), sin(angle), -sin(angle), cos(angle)]` with translation `(e,f)`; the pivot is the element's own origin `(e,f)`; width/height never change.
- Duplicated elements carry `sourceId: <id of the original>` (copy marker; **guess**: harmless, omit).
- Groups: no per-group member list or parent. A display lists **all** its groups in `groupTags`, **innermost first, outermost last** (`[inner, outer]`; verified: two rects in `[D, B]`, a third in `[B]`, so D is nested in B). Grouped displays have **no** `groupTag` key; ungrouped ones keep `groupTags:[]` plus the lone per-element `groupTag` uuid (above). Group tags are `g-<uuid>`; `groupData[tag] = {groupName:"", groupTag, visible:true, enableTransform:true, zOrder, rotation:0}`. The group `zOrder` (inner 4, outer 5) sits between its members' zOrders (3,4 and 5) and the free element (6): looks like the group takes the z slot of its top member **(guess)**. Group `rotation` was 0 on both groups and members' angles untouched: how a rotated group is stored is **unseen** (the older `-12.566` suggests radians **(guess)**).
- Bindings ignore groups: every grouped member is listed individually in `displayIds` of its binding (the 5 elements are split across a VECTOR_ENGRAVING binding with 4 and a VECTOR_CUTTING binding with 1), and members of one group may use different bindings by their layer.

### Fill / stroke / layer
- Engrave-area shape: `fill.visible=true`, `stroke.visible=false`, `isFill=true`, `isClosePath=true`. **Caveat:** in `unknowns`, RECT/CIRCLE/STAR/POLYGON have `fill.visible=true`, `stroke.visible=false` and `isFill=false` (the app's default look for a new primitive) yet are bound to VECTOR_ENGRAVING (score): `fill.visible`/`isFill` are display state, not the processing; `isFill` seems to be set only for imported/converted fills (guess). Line shape (cut/score): `fill.visible=false`, `stroke.visible=true`, `isFill=false`.
- The colour layer (`layerTag`) does **not** decide the processing; the device bindings do (below). In the samples red lines were bound to Vector Engraving (score) in one project and to Cutting in another. Scriber layer -> one xTool colour per layer is a natural mapping, but the processing must be written to the bindings.

## 3. Processing

Everything lives in `devices/device-<code>-<n>.json`:
```
{ "id":"JS002-1","deviceCode":"JS002","extId":"JS002","extName":"M2","power":[10],
  "processing": { "<canvasId>": { "id":"<canvasId>","activeMode":"LASER_PLANE",
      "modes": { "LASER_PLANE": { "ignoredDisplayIds":[], "data":{...}, "profileRefs":[...], "patches":{...}, "bindings":[...] } } } } }
```
`power:[10]` = laser module watts. Other `activeMode`s (`LASER_ROLLER`, `LASER_CYLINDER`, ...) are **(unseen)**.

### Processing types and `profiles.json`
`profiles.json` has exactly three profiles in all samples, one per type, with the default `values`:
- `VECTOR_ENGRAVING` = score/line engrave: `power:1, speed:20, repeat:1, laser:"laser_blue_10W", kerfDistance:0, processHead:"LASER", enableKerf:false, powerCutoffType:"FOLLOW_ACTUALLY_POWER", powerCutoffValue:3, airPump:15`.
- `FILL_VECTOR_ENGRAVING` = area engrave: adds `density:100` (lines per cm; 100 = 0.1 mm, goes with `dpiX/dpiY:254`), `needGapNumDensity:true, bitmapScanMode:"zMode", scanAngle:0, angleType:2, crossAngle:false, outlineTrace:false, ramp:false, ramp_length:0.5, ramp_min_power:1, ramp_outer_edge:false, fill_type:"regular", offset_fill_direction:"IN_TO_OUT"`; `speed:80`, `airPump:15`.
- `VECTOR_CUTTING`: `speed:16`, `cuttingDrop:false, sinkingMethod:"one", firstCuttingDropValue:1, cuttingDropValue:1, descentIntervalDescent:1, descentPerStep:1, enableBreakPoint:false, breakPointGenMode:"auto", breakPointSize:0.5, breakPointMode:"count", breakPointCount:2, breakPointDistance:100, breakPointPower:0, enableKerf:false, enableOverCut:false, overCutDistance:0.5, powerCutoffValue:15, airPump:100`.
- `BITMAP_ENGRAVING` (image engraving): appears as a fourth profile only when the project has a bitmap; values in 2d.
Profile id format: `profile:<12 random alphanumerics>`.
All three profiles were present even when a type had no element (the app writes all).

### Patches (the real numbers)
`modes.LASER_PLANE.patches[<patchId>] = {id, profileId, source, material?, overrides}`; `overrides` is the full set of values for that profile (a superset of or equal to `values`) and wins over the profile. Effective value = `profiles.values` overlaid with `overrides` (what `xs.rs` does).
- `power`: percent 0..100 (the profile default `1` is a placeholder). `speed`: mm/s. `repeat`: passes. `density`: lines/cm. `airPump`: 0..100 (%; cut default 100, others 15). `laser`: `laser_blue_10W`.
- `source: "material"` + `material: {materialType:"official", materialId, paramSource:"official", paramSchemeId, planType:"official"}`: values came from xTool's library (ids belong to xTool's catalogue, `paramSchemeId` per process).
- `source: "custom"`: user edited; no `material` key. `unknowns-3` (device `data.material` 0, no thickness): one patch on the `VECTOR_CUTTING` profile, `overrides` = the full profile value set with `power:21, speed:99, repeat:5` changed (others like `airPump:100`, `laser`, break-point/kerf fields as in the default); the patch is referenced by `patchIds:[<id>]` on a new base binding on that profile holding the single element (the other four keep a patch-less binding), and each of the other two profiles still gets the usual `shadowOf` bindings for both base bindings. So custom values need no `material` object and no `data.material`. Values can also carry `matrixInfo` (material-test grid with per-cell power/speed and a preview URL; irrelevant, omit).
- `patches` may be `{}` and a base binding may have **no** `patchIds` key at all (`unknowns`: nothing customised, `material` 0, `thickness` null, `focalLen` null: values then come from `profiles.json`). Several patches may share one `profileId` (different power for different element groups).

### Material and job data: `modes.LASER_PLANE.data`
`material` (xTool material id, e.g. 1 = a generic default; 1423 = a catalogue entry), `thickness` (mm, floats like 3.0602507), `processPath:"auto"`, `pathPlanning:"auto"`, `fillPlanning:"separate"`, `scanDirection:"topToBottom"`, `enableRemoveCommonEdge:false`, `lineSpacing:0.025`, `focalLen` (mm; app-computed from thickness, e.g. 29.119 at 3.06 mm vs 17.503 at 14.68 mm: **guess** it is bed distance minus thickness), nulls (`perimeter, diameter, focalLength, distance, glbModel`), `fanGear:0, purifierGear:0, purifierV3Gear:0, ap2miniPurifierGear:0, backpackPurifierGear:75|100`, `precautionCodes:["FLAMMABLE","TURN_ON_AIR_PUMP","ODOR_PROTECTION"]`, `xcsUsed:[...]`.

### Bindings: element -> processing
Each binding: `{bindingId:"binding:<12>", canvasId, mode:"LASER_PLANE", baseProfileId, displayIds[], patchIds[], shadowOf?}`.
- A **base binding** (no `shadowOf`) lists in `displayIds` the elements processed with `baseProfileId` and in `patchIds` the patch(es) holding the values. This is the element -> type assignment.
- **Shadow bindings** (`shadowOf: <base bindingId>`, `displayIds: []`) carry the *other two* profiles' patches for the same element set, so switching the type in the UI keeps their values. In every sample each base has two shadows (one per other type), sharing patch ids with other bindings. Seen again in `unknowns` (one base on VECTOR_ENGRAVING with 7 displays + two empty shadows, without `patchIds`). Whether shadows are mandatory is **unknown** (guess: optional; if you write them, mimic the pattern).
- Stale ids: bindings and `ignoredDisplayIds` kept ids of deleted displays in all samples. Harmless; do not rely on referential integrity, but write none.
- `ignoredDisplayIds[]`: elements that are in the project but not processed (the user marked reference outlines "don't process"). This is xTool's no-output; such a display was *also* listed in a base binding. Maps to Scriber's No output.
- Output order: no explicit order field. xTool orders per processing type and by layer `order`/`zOrder`; `fillPlanning:"separate"` and `pathPlanning:"auto"` let the planner decide **(guess)**. Scriber's layer order -> `layerData.order` and `zOrder` is the best available mapping.

## 4. Minimal one-rectangle cut (derived, made-up ids)

Files: `.format`, `meta/persistence-meta.json`, `project.json`, `profiles.json`, `canvases/C.json`, `canvases/C/displays-0.json`, `devices/device-JS002-1.json` (cover and vectors omitted; **whether xTool Studio accepts a file without cover is untested, add a tiny PNG if it complains**).

`project.json` (ids: `P` = `projectId` = `projectTraceID`, `C` = canvas id):
```json
{"__v2__":true,"version":"2.0.0",
 "schemaMeta":{"schemaVersion":"2.1.0","format":"directory","migratedFrom":"1.0","migratedAt":1700000000000},
 "projectId":"P","projectTraceID":"P","projectName":"Test","activeCanvasId":"C","activeDeviceId":"JS002-1",
 "versionInfo":{"source":"pc","appVersion":"1.9.11","savedAt":1700000000000,"ua":"Scriber","minRequiredVersion":"2.6.0","appMinRequiredVersion":"","webMinRequiredVersion":""},
 "created":1700000000000,"modify":1700000000000,
 "modules":{"canvases":["C"],"devices":["JS002-1"]},
 "cover":"resources/project-cover.png","customProjectData":{"projectTraceID":"P"}}
```
(drop `cover` if no PNG is written; `ua` string is informational.)

`profiles.json`: the three profile objects from section 3, verbatim (only ids invented, e.g. `profile:Aaaaaaaaaaaa` ...). Minimum for the example: the VECTOR_CUTTING one; write all three to be safe.

`canvases/C.json`: as section 2 with `layerData {"#fe0002":{"name":"#FE0002","order":1,"visible":true}}`, `groupData {}`, `batchData {"entryType":"BATCH_TEMPLATE","bindings":[]}`, `chunkLayout {"displayCount":1,"chunkCount":1,"chunkIndexes":[0]}`.

`canvases/C/displays-0.json` (50 x 30 mm at x=10, y=20; dPath in world coordinates):
```json
{"canvasId":"C","chunkIndex":0,"displays":[{
 "id":"D1","name":null,"type":"PATH","x":10,"y":20,"angle":0,
 "scale":{"x":1,"y":1},"skew":{"x":0,"y":0},"pivot":{"x":0,"y":0},"localSkew":{"x":0,"y":0},
 "offsetX":0,"offsetY":0,"lockRatio":false,"isClosePath":true,"zOrder":1,"groupTags":[],
 "layerTag":"#fe0002","layerColor":"#fe0002","visible":true,"originColor":"#ff0000","enableTransform":true,
 "visibleState":true,"lockState":false,"resourceOrigin":"","customData":{"tabBreaks":{},"startPoint":{}},
 "rootComponentId":"","minCanvasVersion":"0.0.0","alpha":1,
 "fill":{"paintType":"color","visible":false,"color":0,"alpha":1},
 "stroke":{"paintType":"color","visible":true,"color":16711680,"alpha":1,"width":0.3,"cap":"butt","join":"miter","miterLimit":4,"alignment":0.5},
 "effects":[],"transform":[1,0,0,1,0,0],"width":50,"height":30,"isFill":false,
 "lineColor":16711680,"fillColor":"#ff0000",
 "points":[],"fillRule":"nonzero","graphicX":0,"graphicY":0,"isCompoundPath":false,"pathCornerData":null,
 "dPath":"M10 20H60V50H10Z"}]}
```
(The sample bbox placed `offsetX/Y` = `e/f` = translation; with an identity transform and world dPath they are 0. Fine **if** the app tolerates `x,y` != `e,f`; in the samples with dPath at the origin `x == e`. If it misbehaves, translate the path to its bbox min and put `e,f` = bbox min.)

`devices/device-JS002-1.json`:
```json
{"id":"JS002-1","deviceCode":"JS002","extId":"JS002","extName":"M2","power":[10],
 "processing":{"C":{"id":"C","activeMode":"LASER_PLANE","modes":{"LASER_PLANE":{
  "ignoredDisplayIds":[],
  "data":{"material":1,"thickness":3,"perimeter":null,"diameter":null,"processPath":"auto","pathPlanning":"auto","fillPlanning":"separate","scanDirection":"topToBottom","enableRemoveCommonEdge":false,"lineSpacing":0.025,"focalLen":29.119,"focalLength":null,"distance":null,"fanGear":0,"purifierGear":0,"precautionCodes":["FLAMMABLE","TURN_ON_AIR_PUMP","ODOR_PROTECTION"],"glbModel":null,"purifierV3Gear":0,"backpackPurifierGear":0,"ap2miniPurifierGear":0,"xcsUsed":["glbModel","purifierV3Gear","backpackPurifierGear","ap2miniPurifierGear","axis_y_transmission_ratio_roll_material"]},
  "profileRefs":["profile:Cut"],
  "patches":{"patch:X":{"id":"patch:X","profileId":"profile:Cut","source":"custom","overrides":{ /* full VECTOR_CUTTING values from profiles.json with power:100, speed:6, repeat:3 */ }}},
  "bindings":[{"bindingId":"binding:B","canvasId":"C","mode":"LASER_PLANE","baseProfileId":"profile:Cut","displayIds":["D1"],"patchIds":["patch:X"]}]}}}}}
```
`source:"custom"` (no `material` object) avoids inventing xTool catalogue ids. Whether xTool then shows the correct "no material" state is untested.

Must be generated: `projectId`/`projectTraceID` (uuid v4, equal), canvas id (uuid), display ids (uuid), group tags (`g-<uuid>`), profile ids (`profile:<12 alnum>`), patch ids (`patch:<12>`), binding ids (`binding:<12>`), timestamps (ms since epoch: `migratedAt`, `savedAt`, `created`, `modify`), `vectorHash` (**sha256 hex of the path string**, verified: it equals the key and `size` = UTF-8 length of the string, in `index.json` `entries[hash].size`). Nothing resembling a zip-wide checksum or signature was found. `customData.from.templateId` is `sha256:...` of unknown input (appears on shapes from a template; omit).

## 5. Unknowns and risks

Resolved by the `unknowns` sample: native `RECT`/`CIRCLE`/`STAR`/`REGULAR_POLYGON` exist and their fields (section 2b); `fill.visible`/`isFill` do not decide processing; `layerTag` (not `stroke.color`) carries the layer; profiles.json is unchanged (still exactly three profiles, no `BITMAP_ENGRAVING` unless a bitmap exists); project/device/cover files show nothing new (cover has a `.meta.json` sidecar `{"ref","metadata":{"kind":"image","source":{"type":"workspace","value":"project-cover.png"},"mimeType":"image/png"}}`).

Resolved by `unknowns-2`: text storage (2c: `style`, `fontData`, `charJSONs` outlines stored, `fontSize = 72 * scale`), bitmap storage (`resources/<sha256>.png` + sidecar, size/dpi from `scale`), image adjustments, `BITMAP_ENGRAVING` profile and the `bitmapMode` enum (2d). Resolved by `unknowns-3` (section 2e): `angle` in degrees, clockwise positive, `transform = rotation matrix about the top-left`, `x,y = offsetX/Y = e,f` (rotated RECT only); nested groups via `groupTags` innermost first; groups do not affect bindings; a custom-values patch. Not in that sample, still unknown: rotated group, rotation of PATH/STAR/TEXT/BITMAP, library material (the part of A.1 and the group/material parts below), bitmap crop, other `bitmapMode` names, multi-line/aligned text.

A. Still unknown
1. Rotation of STAR, PATH, TEXT, BITMAP (RECT, CIRCLE, REGULAR_POLYGON seen; `pivot` always 0), group `rotation` unit and whether rotating a group rewrites members' `angle`/`transform`.
2. QR, bitmap crop, further `bitmapMode` names, multi-line text, patches with a library `material` object (`unknowns-3` had none; only the shape in section 3, seen in earlier samples).
3. `CIRCLE` scale 0.01 oddity (seen again in `unknowns-3`: consistent, but cause unknown); orientation of `REGULAR_POLYGON`; per-corner radii in `primitiveCornerData`.
4. Whether an element in no binding is accepted; whether shadow bindings are required.
5. Machine bed origin versus canvas origin: a file has no bed reference; export at its own x,y and let the user position the job (guess).
6. Meaning of `batchData.entryType`, `displayProcessConfigMap`, `rulerGuide`, `xcsUsed`, `chunkLayout` (multi-chunk), `layerData` name rules and non-palette colours.
7. Output ordering rules; `focalLen` formula (`null` when no material/thickness is set).

B. Risks that could make xTool Studio reject or "repair" a file
1. Wrong `type` strings or missing type fields: for native types copy the full field list above; when unsure use `PATH`.
2. Missing optional-looking fields (the app may crash instead of defaulting): copy the full common field list verbatim.
3. `minRequiredVersion: "2.6.0"` in `versionInfo`: copy it.
4. Referential integrity: a display id in `bindings` but absent from displays is tolerated; a display in no binding is **unknown** (xTool binds ignored ones too); Scriber binds every processed element and lists no-output ones only in `ignoredDisplayIds`, in no binding, so they can never be processed. If xTool turns out to need a binding, add them to a base binding as well.
5. Float formatting: <= 6 decimals for geometry in native fields; path strings carry full doubles; colours ints.
6. Zip: deflate, directory entries, `.format` last; other layouts untested.
7. Account data: none in the samples (`ua` string only); never copy `projectId`s or `ua` from user files.
8. Text/bitmap/group/rotation/multi-canvas support in export needs a sample each; a second sample is requested below.

**Extra sample that would settle most of A:** one canvas with (a) a text element (one line, any font, plus one in a different size), (b) one imported PNG with greyscale and one with dither/threshold set, bound to Image Engraving, (c) a group of two shapes nested in another group, (d) a RECT, a CIRCLE (drawn by dragging, then a perfect circle by typing size) and a polygon each rotated by a round angle (45 and 90, saved separately if possible), (e) one element with custom power/speed/passes and a material from the library (to see patches).

## 6. Mapping

### Scriber -> .xs (export)
| Scriber | .xs |
|---|---|
| Document | one canvas (`title "{panel}1"`), one device (`JS002-1` for xtool-m2) |
| Rectangle (item recognised as axis-aligned 4-vertex rect) | native `RECT` (`radius` from fillet if all corners equal; else `PATH`); optional: still **untested** whether the app accepts a hand-written `RECT`; `PATH` stays the proven fallback. A rotated rect could be written as `RECT` with `angle` (degrees, clockwise) and `transform [cos,sin,-sin,cos,x,y]` about its top-left `x,y` (2e), but only a read-only sample proves this: stay conservative, bake the rotation into a `PATH` until a hand-written file is opened in the app |
| Circle (single full arc contour) | native `CIRCLE` is possible (`width = height = 2r`); because of the scale 0.01 oddity, **prefer `PATH`** with four cubic Béziers until tested |
| Item contour(s) + bulge arcs | `PATH.dPath`: `M x y`, straight segments `L`/`H`/`V`, bulge arcs as SVG `A rx ry 0 large sweep x y` (r = chord/(2 sin(theta/2)), theta = 4 atan(bulge), sweep from sign) **(arcs not seen in samples; the app writes `C`; to be safe convert arcs to cubic `C` segments)**; closed contour ends `Z`; several contours -> one `dPath` with `isCompoundPath:true`, `fillRule:"evenodd"` |
| bbox, position | `x,y,width,height` = bbox of the written path; `transform=[1,0,0,1,0,0]`, `scale 1,1`, `angle 0` (bake item rotation into the points; with a native `RECT` the rotated `x,y` is the unrotated top-left, not the bbox) |
| Layer (colour) | `layerData` entry per layer (`#rrggbb` lowercase) with `order` and `name` = the Scriber layer name; element `layerTag/layerColor` and `stroke.color`/`originColor`/`fillColor`; use the layer's canvas colour, else op colour (cut red `#fe0002`, score blue `#00befe`, engrave black `#000000`, no output grey). xTool keys layers by colour, so when an earlier exported layer already has the colour, it is nudged to the nearest free one (one channel ±1, then ±2, …): every Scriber layer stays its own xTool layer |
| Op Cut | binding on VECTOR_CUTTING; element stroke visible, no fill |
| Op Score | binding on VECTOR_ENGRAVING; stroke visible |
| Op Engrave (closed paths) | binding on FILL_VECTOR_ENGRAVING; `fill.visible=true`, `stroke.visible=false`, `isFill=true`; open engrave path -> score |
| Op No output | own colour layer; its elements are displays listed in `devices/device-<code>-1.json` → `processing[<canvasId>].modes.LASER_PLANE.ignoredDisplayIds`, and in **no** binding (xTool's own samples also bound them; unbound ignored displays are untested in the app). Left out with `include_no_output: false` |
| Hidden layer | not exported |
| Setting power %, speed mm/s, passes | patch `overrides.power`, `.speed`, `.repeat` (profile values otherwise as in profiles.json) |
| Engrave line interval mm | `overrides.density = 10 / interval` (lines per cm); `dpiX = dpiY`: samples only show density 100 with dpi 254, so write `round(density * 2.54)` (guess); `lineSpacing` in `data` stays 0.025 |
| Air assist | `airPump` (0..100; default 100 cut, 15 others) |
| Material (library) thickness | `data.thickness`; `data.material` 1 (generic default, seen) or the xTool id if known; `data.focalLen` as guess above |
| Machine module (diode 10 W) | `laser:"laser_blue_10W"` in overrides, `device.power:[10]` (module string pattern `laser_<source>_<watts>W`, source blue/red/co2 per the importer) |
| One patch per (layer, op) group | distinct patch + base binding per setting set; elements sharing a setting share the binding |
| Image (greyscale) | `BITMAP` display (2c): PNG bytes to `resources/<sha256>.png` + `.meta.json`, `originWidth/Height` = pixels, `s` = mm per pixel (= item width_mm / pixels), `transform [s,0,0,s,x,y]`, `x,y` top-left, `width/height` mm, `dpi` 25.4/s, adjustments at the defaults; add the 4th `BITMAP_ENGRAVING` profile (2d) to `profiles.json`/`profileRefs`; bind the image in a base binding on it (no shadows); mode `grayscale` (leave the Scriber greyscale as the PNG; `bitmapMode` per Scriber dithering if it has one: `Jarvis`/`Bayer`/`Stucki`/`Atkinson`/`Sierra`); Scriber line interval -> `density` as for engrave. Untested in the app. |
| Text | convert to outlines (path) at export (xTool itself keeps glyph outlines in `charJSONs`, so outline PATHs are a faithful representation; native `TEXT` needs `fontData`, do not write); outline text of an engrave layer: fill type as for shapes |
| Groups | `groupData` + `groupTags` (`[innermost, ..., outermost]`, no `groupTag` key; each member still bound individually; optional, export flat) |
| Notes/guides/dimensions | not exported |

### .xs -> Scriber (import)
| .xs | Scriber |
|---|---|
| each display `RECT`/`CIRCLE` | rect contour (rounded by `radius`; arcs as bulge) / ellipse at (`offsetX/Y`, `width`, `height`) after `transform` (apply `scale` if not 1: the CIRCLE oddity means trust `width`/`height`; if `angle` != 0 rotate the box about its top-left `(e,f)` by `angle` degrees clockwise, i.e. use `transform`; `x,y` is not the bbox then) |
| `STAR`/`REGULAR_POLYGON` | generate vertices from `count`, `starInnerScale` in the `oWidth` x `oHeight` box at `offsetX/Y`, then fit to the tight bbox `x,y,width,height` (orientation guess: first vertex up) |
| each display (`PATH`/`LINE`) | one item; world geometry = `transform * dPath` (resolve `vectorRef` via `vectors/svg`), parse `M L H V C Q A Z` (cubics fitted to arcs as the SVG import does); `isCompoundPath` / several `M` -> several contours; `fillRule` evenodd/nonzero |
| binding `baseProfileId` type | op: VECTOR_CUTTING -> cut, VECTOR_ENGRAVING -> score, FILL_VECTOR_ENGRAVING -> engrave; bitmap -> image; element in `ignoredDisplayIds` -> no output |
| display `layerTag` + `layerData` | Scriber layer (name, colour); but the op comes from the binding, so one xTool colour can yield several Scriber layers (colour x op) |
| patch `overrides` (over `profiles.values`) | layer setting: power, speed, `repeat`->passes, `density`->line interval (10/density) |
| `data.material`, `.thickness` | material (existing reader `xs.rs` makes a library entry named after the file) |
| `device.power`, `laser` | machine module |
| `groupTags`/`groupData` | groups (name `groupName`); `zOrder` -> stacking |
| fill visible & `isFill` | engrave area; stroke only -> line |
