# Drawing

Every tool is on the ribbon's **Home** tab (Draw panel), has a one-key shortcut, and shows a
[command card](./the-screen#command-card) that tells you what to click next. Esc stops a tool.

## Tools

| Tool | Key | How |
|---|---|---|
| Line | L | Click point after point; each segment is its own line. Done or Enter stops |
| Polyline | P | Like Line, but one connected shape. *Close shape* joins the last point to the first |
| Rectangle | R | See the ways below. *Rounded corners…* gives it a corner radius |
| Circle | C | See the ways below |
| Arc | A | See the ways below |
| Polygon | G | Click the centre, then the size. *Sides* on the card sets the number of sides |
| Text | T | Click where it goes, type the text, *Place text*. Font (any installed one), bold and italic are in the properties panel |

## Ways of drawing (variants)

Circle, Arc, Rectangle and Polygon can be drawn in more than one way. Pick the way with the small
arrow under the ribbon button, on the command card or in the right-click menu. Scriber remembers the
last way you used.

| Tool | Ways |
|---|---|
| Circle | Centre and radius · 2 points (diameter) · 3 points |
| Arc | 3 points · Start, centre, end · Centre, start, end · Start, end, radius |
| Rectangle | 2 corners · Centre and corner · 3 points (rotated) |
| Polygon | Size to a corner · Size to a side |

## Command cards

Each tool's card shows its prompt and options.

<p>
<img src="/screenshots/cards/card-l.webp" width="380" alt="The Line command card">
<img src="/screenshots/cards/card-r.webp" width="380" alt="The Rectangle command card with its ways of drawing and corner options">
<img src="/screenshots/cards/card-c.webp" width="380" alt="The Circle command card with its three ways of drawing">
<img src="/screenshots/cards/card-a.webp" width="380" alt="The Arc command card with its four ways of drawing">
<img src="/screenshots/cards/card-g.webp" width="380" alt="The Polygon command card with the number of sides and two ways of sizing">
<img src="/screenshots/cards/card-t.webp" width="380" alt="The Text command card">
</p>

## Exact sizes

Click the first point, then type: the number goes into the first measurement (length, width,
radius, …), <kbd>Tab</kbd> moves to the next, <kbd>Enter</kbd> places it. See
[dynamic input](./the-screen#dynamic-input). A size you type is kept as a dimension of the shape.

With the command line shown you can also type points: `20,30` (absolute), `@15,0` (relative to the
last point) or `@40<30` (40 mm at 30°). A plain number uses the pointer's direction.

## Helpers

- **Object snaps** (OSNAP) catch ends, middles, centres, quadrants, crossings, perpendicular and
  tangent points, and the nearest point on a shape.
- **Tracking** (OTRACK, POLAR) shows dashed lines to line up with points you hovered over earlier.
- **Guide lines** (Annotate → Guides): endless lines, rays, centre lines and centre marks. They help
  you place things and are never lasered or exported. *Show guide lines* hides them.
- **Note dimension** (Annotate): a measurement drawn for people, on a *No output* layer. It does not
  drive the shape; for that use a [locked size](./constraints).

## Images, SVG and DXF files

- File → **Import image…** places a PNG, JPEG or other picture. It is turned to greyscale and is
  engraved.
- File → **Import SVG…** reads vector drawings. The stroke colour picks the layer: red = cut,
  blue = score, black = engrave, grey or green = no output, any other colour = cut. A filled closed
  shape is engraved. You can also paste SVG text straight onto the canvas.
- File → **Import DXF…** reads drawings from CAD programs (or drop a `.dxf` on the canvas). See
  below.

### Importing DXF

A dialog lists the DXF's layers, each with its colour and how many items it holds, and asks what
the laser does with each: **Cut**, **Score**, **Engrave**, **No output** or **Skip** (leave it out).
Each layer you import becomes a Scriber layer with the same name. The first guess comes from the
layer's name when it says what to do ("Cut", "Engrave", …), else from its colour: red cut, blue
score, black and dark grey engrave, grey no output, white (AutoCAD's colour 7) and any other colour
cut. Layers that are turned off in the DXF start as Skip. Scriber remembers your choice for each
layer name, so the next DXF from the same CAD program comes in the same way. A layer that holds
several colours is split into one row per colour.

- Units come from the file (`$INSUNITS`: mm, cm, inch, …). A file without units is read as
  millimetres; the dialog shows which.
- The DXF origin lands on the bottom-left corner of the material, unless you tick **Place at**.
- Lines, arcs, circles and polylines come in exactly (arcs stay arcs). Ellipses and splines are
  turned into arcs that stay within 0.01 mm of the curve.
- Lines and arcs that touch end to end are joined into one shape (closed when they come round), and
  shapes inside other shapes become one compound path, so holes stay empty when engraved.
- Blocks (INSERT) are placed with their position, rotation, scale and arrays, also blocks in blocks.
- TEXT and MTEXT become text items in the default sans-serif font (the DXF's own font is not
  matched).
- Points, hatches, dimensions, leaders and images are left out; the dialog lists what was skipped.
