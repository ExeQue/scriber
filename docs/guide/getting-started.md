# Getting started

Scriber is a 2D drawing program for laser cutters. You draw shapes in millimetres on a sheet of
material, say what the laser should do with each shape, and export a file for your laser software.

## Install and run

There are no release downloads yet. Build Scriber from source with [Rust](https://rustup.rs):

```sh
git clone https://github.com/ExeQue/scriber.git
cd scriber
cargo build --release
./target/release/scriber            # opens the editor
./target/release/scriber my-box.scrbr   # opens a file
```

Scriber runs on Linux and Windows. The same program also works from the command line:

```sh
scriber --help             # all commands
scriber agents list        # AI agents found on this computer
scriber agents add         # connect Scriber to them (see "Connecting agents")
```

## Your first drawing: a box with holes

A plate of 100 × 60 mm with four 6 mm holes, cut out of 3 mm plywood.

1. **Set the material size.** With nothing selected, the properties panel on the right shows
   **Material** with *Width* and *Height*. Make the sheet big enough, for example 200 × 150 mm.
   If you have picked a machine (Laser tab), **Use work area** sets it to the machine's bed.
2. **Draw the plate.** Press <kbd>R</kbd> (or Home → Rectangle). Click where the first corner goes.
   Now type `100`, press <kbd>Tab</kbd>, type `60` and press <kbd>Enter</kbd>.
   The width and height you typed stay locked as dimensions.
3. **Draw a hole.** Press <kbd>C</kbd> (Circle). Click the centre, type `3` (the radius) and press
   <kbd>Enter</kbd>. Want to type the diameter instead? Pick *Diameter* on the command card or the
   right-click menu.
4. **Make four of them.** Select the circle, then Home → Array (<kbd>Y</kbd>) and follow the card:
   2 rows, 2 columns and the spacing between them. Or use Copy (<kbd>K</kbd>) three times.
5. **Check the layer.** New shapes go on the current layer. A new document starts with the layers
   *Cut*, *Score*, *Engrave* and *No output*, and *Cut* is current, so everything you drew will be cut.
6. **Look at it.** Press <kbd>V</kbd> for the laser preview: the plate shows as a loose piece with
   four open holes. Press <kbd>V</kbd> again to go back to editing.
7. **Save and export.** <kbd>Ctrl+S</kbd> saves a `.scrbr` project. File → **Export SVG for the
   laser…** writes a file your laser software can open; xTool users can also export an xTool Studio
   project (see [Exporting](./exporting)).

Esc cancels a running tool and takes you back to selecting. Ctrl+Z undoes.

## Units and directions

- Everything is in millimetres.
- The origin (0, 0) is the top-left corner of the material. x grows to the right and **y grows
  down**, as in SVG files.
- Angles are in degrees; positive turns clockwise on the screen.

## Next

- [The screen](./the-screen): what all the panels and toggles are.
- [Layers and laser operations](./layers): cut, score, engrave and no output.
- [Machines and materials](./machines-and-materials): power and speed per material.
