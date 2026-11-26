# The screen

![Scriber's main window in the light theme: ribbon, document tabs, canvas, layers panel and status bar](/screenshots/theme-light.webp)

*Scriber with a demo project: the ribbon and tabs along the top, the canvas with the material sheet, the layers panel on the right and the status bar at the bottom.*


```
┌ File  Edit  AI agents  Help ───────────────────────────────────────────┐
│ Home │ Annotate │ Parametric │ Arrange │ Laser │ View          (ribbon) │
│ [big buttons] [small buttons] ...                                      │
├─ tabs: box.scrbr • │ lid.scrbr │ + ─────────────────────┬──────────────┤
│                                                       │ Layers       │
│        canvas: the material sheet                      │              │
│        ┌ command card ┐                               │ Properties   │
│        └──────────────┘                               │              │
├────────────────────────────────────────────────────────┴──────────────┤
│ GRID SNAP ORTHO OSNAP POLAR▾ OTRACK INFER        x 120.0  y 45.5      │
└────────────────────────────────────────────────────────────────────────┘
```

## Ribbon

The ribbon under the menu bar holds every tool, grouped in tabs:

| Tab | What is on it |
|---|---|
| **Home** | Draw (line, polyline, rectangle, circle, arc, polygon, text), Modify (move, copy, rotate, scale, mirror, fillet, chamfer, offset, trim, array, delete), Combine (merge, cut out, keep overlap), Undo, the layer for new shapes, Preview and Check |
| **Annotate** | Text, note dimensions, measuring, guide lines, review comments |
| **Parametric** | Geometric constraints, locked sizes (Dimension and *More ▾*), Parameters |
| **Arrange** | Stretch, join, explode, align, break, extend, lengthen, copy laser settings, group, show only selection, select similar, quick select |
| **Laser** | Machine and material, export, Machines & materials… |
| **View** | Zoom, Preview, toggles for the grid, guide lines, comments, measure on hover and the command line, and **Theme ▾** |

Hover a button for its name, a one-line help and its shortcut. Buttons with a small arrow (circle,
arc, rectangle, polygon) have several ways of drawing; see [Drawing](./drawing). The ⌃ at the right
folds the ribbon to just the tab names.

## Themes

View → **Theme ▾** (or the View menu → Theme) picks the colours: **System** (the default) follows
Omarchy's theme on Omarchy and your system's light or dark mode elsewhere; there are also Dark,
Light, Graphite, Paper, Blueprint and High contrast, and theme files of your own. Laser colours stay
red for cut, blue for score and black for engrave, made lighter where the material is dark. See
[Themes](./themes).

## Command card

When a tool runs, a small card on the canvas says what to do next in plain words ("Click the first
corner"), shows the tool's settings and options, and has **Done** and **Cancel** buttons. You never
have to type a command.

## Dynamic input

Once the first point is down, the card and a small box next to the cursor show live measurements:
length and angle, width and height, radius and diameter, an arc's angle. To use exact numbers:

- type a number: it goes into the first field;
- <kbd>Tab</kbd> locks it and moves to the next field (the rest keeps following the mouse);
- <kbd>Enter</kbd> places the point.

A length, width, height, radius or diameter you type stays locked as a dimension of the new shape
(see [Constraints and dimensions](./constraints)).

## Status bar: drawing aids

| Toggle | Key | What it does |
|---|---|---|
| GRID | F7 | Show the grid on the material |
| SNAP | F9 | Round points to the grid |
| ORTHO | F8 | Only horizontal and vertical lines from the last point. Holding <kbd>Shift</kbd> while drawing flips it |
| OSNAP | F3 | Lock onto ends, middles, centres, quadrants, crossings and other points of shapes |
| POLAR | F10 | Track angles from the last point. Pick the angle step (90° to 5°, 15° by default) and extra angles with ▾ |
| OTRACK | F11 | Rest the pointer on a snap point to "acquire" it; dashed guide lines then run from it, and where two cross the cursor snaps |
| INFER | | What you draw on a snap stays attached to it (see [Constraints](./constraints#inferred-constraints)) |

OSNAP, POLAR, OTRACK and INFER are on by default. <kbd>Shift</kbd>+right-click opens a one-time snap
menu for the next point only, with extras such as *Mid between 2 points* and *From* (a base point
plus an offset).

## Right-click menus

Right-click on the canvas:

- **while a tool runs**: Enter, Cancel and the tool's options (for example *Close shape* and *Undo
  last point* for a polyline, *Diameter* for a circle, *Rounded corners…* for a rectangle);
- **with no tool running**: repeat the last command, cut/copy/paste, undo/redo, select all,
  *Dimension*, *Add constraint ▸*, *Comment on this…* and *Show whole material*.

Right-clicking a shape selects it first.

## Selecting

- Click a shape to select it; hovering highlights what a click would pick and, after a moment,
  shows its name, laser operation and size.
- Drag **left to right** for a window selection: only shapes fully inside.
- Drag **right to left** for a crossing selection: anything the box touches.
- <kbd>Shift</kbd>+click removes a shape from the selection.
- Where shapes overlap, <kbd>Shift+Space</kbd> steps through the ones under the cursor.
- Holding <kbd>Ctrl</kbd> with no tool running measures what is under the cursor.

## Panels on the right

- **Layers**: the document's layers; see [Layers and laser operations](./layers).
- **Properties**: exact numbers for the selection (position, size, rotation, layer, constraints).
  With nothing selected it shows the material size and the document notes.
- **Comments**: listed when the document has any; see [Review comments](./comments).

## Command line

For AutoCAD users: View → **Command line** shows a command line at the bottom. Type commands and
aliases (`LINE`/`L`, `CIRCLE`/`C`, `RECTANG`/`REC`, `FILLET`/`F`, `TRIM`/`TR`, …) and points as
`x,y`, `@dx,dy` or `@distance<angle`. While it is shown, letters go to the command line instead of
the one-key shortcuts below.

## Coming from AutoCAD or DraftSight

If you have used AutoCAD or DraftSight, most of this screen will feel familiar.

| You know | In Scriber |
|---|---|
| Ribbon tabs | The ribbon above: Home, Annotate, Parametric, Arrange, Laser, View |
| Command line | View → **Command line**, hidden by default. Aliases such as `L`, `C`, `REC`, `PL`, `TR`, `F`, `O`, `CO`, `RO`; points as `x,y`, `@dx,dy`, `@distance<angle` |
| Command shortcuts | Single keys (<kbd>L</kbd> line, <kbd>M</kbd> move, <kbd>D</kbd> dimension, ...), not typed aliases; see the table below |
| OSNAP, OTRACK, POLAR, ORTHO | The same names in the status bar, with F3, F11, F10 and F8 |
| Dynamic input | The box next to the cursor: type a number, <kbd>Tab</kbd> to the next field, <kbd>Enter</kbd> |
| Grips | Drag the blue grips of a selected shape; see [Editing](./editing#grips) |
| Geometric constraints | `GC...` commands (`GCPARALLEL`, `GCTANGENT`, ...) and right-click → *Add constraint ▸* |
| Dimensional constraints, DIM | `DIM` (or <kbd>D</kbd>) locks a length, radius or angle; `DCLINEAR`, `DCRADIUS` etc. also work |
| Parameters manager | `PARAMETERS` / `PAR`, or Parametric → **Parameters** |
| Window / crossing selection | Drag left to right / right to left |
| Right-click, Enter or Space repeats | Same |

Some things differ:

- There is no model and paper space: one document is one sheet of material.
- Units are millimetres only.
- A layer carries the laser operation (cut, score, engrave or nothing) instead of a colour and
  linetype only; see [Layers and laser operations](./layers).

AutoCAD and DraftSight are trademarks of their owners; Scriber is not affiliated with them.

## Keyboard shortcuts

Single keys start tools while you are not typing and the command line is hidden. Help → Keyboard
shortcuts shows the same list in the app.

| Key | Tool | Key | Tool |
|---|---|---|---|
| L | Line | O | Offset |
| P | Polyline | X | Trim |
| R | Rectangle | Y | Copies in a pattern (array) |
| C | Circle | J | Join |
| A | Arc | E | Explode |
| G | Polygon | W | Stretch |
| T | Text | B | Break |
| D | Dimension | Del | Delete |
| I | Measure distance | V | Preview what the laser makes |
| M | Move | Z | Show the whole material |
| K | Copy | | |
| Q | Rotate | | |
| S | Scale | | |
| N | Mirror | | |
| F | Round corner (fillet) | | |
| H | Cut corner (chamfer) | | |

Other keys:

| Key | Does |
|---|---|
| Esc | Cancel the command (back to Select), or clear the selection |
| Enter / Space | Finish, or repeat the last command |
| Space + drag | Pan the view |
| Shift+Space | Cycle through overlapping shapes |
| Arrow keys | Nudge the selection 1 mm (with Shift 10 mm) |
| Backspace | Delete the selection |
| Ctrl+Z / Ctrl+Y | Undo / Redo |
| Ctrl+S / Ctrl+Shift+S | Save / Save as |
| Ctrl+O | Open |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / Cut / Paste at the pointer (also between windows) |
| Ctrl+Shift+V | Paste in place |
| Ctrl+D | Duplicate |
| Ctrl+A | Select all |
| Ctrl+G / Ctrl+Shift+G | Combine / Break apart |
| Ctrl+N / Ctrl+W | New tab / Close tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| F3, F7–F11 | Drawing aids (status bar) |
