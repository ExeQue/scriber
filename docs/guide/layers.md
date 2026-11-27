# Layers and laser operations

Every shape sits on one **layer**, and the layer says what the laser does with it, as in xTool
Studio. To change what happens to a shape, move it to another layer.

## The four operations

| Operation | Colour | What the laser does |
|---|---|---|
| **Cut** | red | Cuts through the material along the line |
| **Score** | blue | Marks a thin line on the surface |
| **Engrave** | black | Burns the whole area inside closed shapes, text and images. An open line on an engrave layer is scored |
| **No output** | grey | Nothing. For notes, measurements and helpers meant for people |

A new document has one layer of each: *Cut*, *Score*, *Engrave* and *No output*.

**One shape, one operation.** To engrave a coaster's face and cut it out, draw the shape twice
(Duplicate, Ctrl+D) and put one copy on an engrave layer and one on a cut layer.

Holes in a compound shape (see [Combining shapes](./editing#combining-shapes)) stay empty when the
shape is engraved.

## The Layers panel

On the right, above the properties. Per layer:

- the **current layer** radio button: new shapes go there (also the layer pills on the Home tab);
- colour (only on screen; by default the operation's colour), name and operation;
- **show** and **lock** toggles. A hidden layer is not drawn, lasered, checked or exported. Shapes
  on a locked layer are drawn but cannot be selected or changed;
- move up and down, delete (only when the layer is empty and not the last one);
- a fold-out with the layer's own laser settings (below).

![The Layers dialog: each layer with colour, name, show and lock toggles, operation, move and delete buttons](/screenshots/edit-layers.webp)

*Edit layers… opens this dialog with the same controls, plus Add layer.*

**Add layer** adds one with the current layer's operation and makes it current. To move shapes,
select them and pick a layer in the properties panel (*Move to layer* for several).

Text drawn while a Cut layer is current goes on the first Engrave layer; note dimensions and guide
lines go on the first No output layer.

## Laser settings per layer

By default a layer uses the [material library](./machines-and-materials)'s setting for its
operation, for the document's machine, laser module and material. In the layer's fold-out you can
override **power** (%), **speed** (mm/s) and **passes** one by one, and on engrave layers also
**Lines per cm** (the line spacing; 100 lines per cm = 0.1 mm between lines): a ticked box means
the layer's own value, unticked shows the library's value greyed out.

When the material has several **profiles** for the layer's operation (for example *Light* and
*Deep* engraving), the layer has a drop-down to pick one. Your own numbers still win over the
profile's.

On engrave layers a second drop-down picks the **image mode**: how the layer's images are engraved,
in greyscale or with a dither. The list comes from the document's machine (for xTool machines:
Grayscale, Jarvis, Bayer, Floyd, Stucki, Atkinson, Sierra and Passthrough); the first is the
default. Scriber passes the choice on to the machine's software in the export (the `.xs` project for
xTool Studio), which does the dithering, so the preview still shows greyscale. A mode the machine
does not have (after you switch machines) falls back to its default, and the pre-flight check says so.

This is how you get two engraving depths in one job: two engrave layers with different settings or
profiles.
