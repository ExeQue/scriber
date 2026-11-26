# Exporting

Everything that is lasered comes out of the export: shapes on shown cut, score and engrave layers.
Hidden layers, *No output* layers, guide lines and comments are left out.

| Export | Where | What you get |
|---|---|---|
| **SVG for the laser** | File → Export SVG for the laser…, Laser → Export | An SVG in millimetres. Colours follow what laser software expects: engrave black fill, score blue, cut red. One SVG layer per Scriber layer, in laser order: engrave, score, cut |
| **PNG picture** | File → Export PNG…, Laser → Export | A picture of the drawing at 254 DPI |
| **xTool Studio (.xs)** | File → Export xTool Studio (.xs)…, Laser → Export | An xTool Studio project with each layer's processing type, power, speed and passes |

Text is exported as outlines, so the laser software does not need the font.

## xTool Studio projects

The `.xs` export writes one xTool colour layer per Scriber layer, with the settings from your
[material library](./machines-and-materials) (and the layer's own numbers on top):

- Cut → vector cutting, Score → vector engraving, Engrave → fill engraving (lines per cm become
  xTool's density). Open shapes on an engrave layer are scored.
- Images are engraved as bitmaps. Rotated images are skipped, and the export says so.
- The machine must be an xTool machine with a device code (all built-in xTool machines have one).
  With no machine chosen, the project is written for an M2 with the 10 W laser, with a note.

The format is reverse-engineered; see [the .xs format](../xs-format) for details. Open the result
in xTool Studio and check the settings before you run the job.

## Project files

<kbd>Ctrl+S</kbd> saves the document as a `.scrbr` project: everything, including layers,
constraints, comments and the chosen machine and material. See [File formats](../reference/file-formats).
