# File formats

| Extension | What | Made by |
|---|---|---|
| `.scrbr` | A Scriber project | File → Save |
| `.scrlib` | Machines and materials to share | Machines & materials → Export ▾ |
| `library.json` | Your own material library | The app, in its config folder |
| `machines/*.json` | Built-in machines | Shipped with Scriber |
| `.svg`, `.png` | Exports for laser software | File → Export |
| `.xs` | xTool Studio project | File → Export xTool Studio (.xs)… |

## `.scrbr` projects

A zip archive:

```
my-box.scrbr
├── document.json      everything you drew, as readable JSON
└── images/
    └── <id>.png       imported images (greyscale originals)
```

`document.json` holds the material size, notes, layers, items, constraints and parameters, groups,
review comments, and the ids of the chosen machine, laser module and material (the settings
themselves stay in the library). It carries a `format` number (now 2); a file from a newer Scriber
is refused with a request to update, and older files are converted when opened.

Because the JSON is plain text, `document.json` diffs well in version control. Images are kept as
separate PNG files rather than inside the JSON.

Coordinates are in millimetres with the origin at the top-left of the material and y growing down.
Every shape is stored as one or more contours of vertices `{x, y, bulge}`, where `bulge` describes
an arc to the next vertex (0 = a straight segment), as in DXF.

## `.scrlib` library files

JSON with `format: "scriber-library"`, a `version` (now 1) and lists of `machines` and `materials`.
See [Make your own material library](../library/make-a-material-library) for every field and
[Share a library](../library/share-a-library) for importing.

## `library.json`

Your library, in `<config>/scriber/library.json` (`$SCRIBER_LIBRARY` overrides the path). Same
machine and material objects as a `.scrlib`, plus `removed_machines` / `removed_materials` for
built-ins you deleted. It holds only what you added or changed; built-in machines you did not touch
always come fresh from the app.

## Machine and material JSON

Described in [Make your own material library](../library/make-a-material-library#machines). The
built-in machines are the files in the repository's
[`machines/`](https://github.com/ExeQue/scriber/tree/main/machines) folder, one machine per file
named after its `id`.

## Exports

- **SVG**: millimetre units; engrave black fill, score blue stroke, cut red stroke; one
  Inkscape-style layer group per shown Scriber layer, in laser order (engrave, score, cut). Text is
  converted to outlines.
- **PNG**: a picture of the drawing.
- **`.xs`**: see [Exporting](../guide/exporting#xtool-studio-projects) and the
  [.xs format notes](../xs-format).
