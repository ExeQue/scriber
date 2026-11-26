# Make your own material library

Scriber ships without material settings, because the right power and speed depend on your machine,
your laser module and the exact sheet you buy. This page shows how to build a library, in the app or
as a file you can keep, share and put in version control.

## In the app

The quickest way: Laser → **Machines & materials…** → Materials → **Add**, fill in the form and press
**Save**. See [Machines and materials](../guide/machines-and-materials). A good routine:

1. Cut a small test grid on a scrap of the material, with different speeds and powers.
2. Enter the setting that worked for cut, score and engrave, for the machine and laser you used.
3. Write in **Source** what you tested and when, so you can trust the numbers later.

You can also [import settings from xTool Studio](../importing-xtool-materials) or ask an AI agent to
add them (`save_material`, `set_material_setting`).

## As a file

A library file (`.scrlib`) is plain JSON. Write one in any text editor and load it with
**Import ▾ → Scriber library file…** in the Machines & materials window:

```json
{
  "format": "scriber-library",
  "version": 1,
  "machines": [],
  "materials": [
    {
      "id": "birch-plywood-3mm",
      "name": "Birch plywood 3 mm",
      "category": "plywood",
      "thickness": 3,
      "color": [214, 180, 130],
      "notes": "From the hardware store, 3-ply.",
      "source": "My own test grid, 2026-11-20.",
      "settings": [
        { "machine": "xtool-m2", "laser": "diode-20w", "operation": "cut",
          "power": 100, "speed": 6, "passes": 1 },
        { "machine": "xtool-m2", "laser": "diode-20w", "operation": "score",
          "power": 30, "speed": 100, "passes": 1 },
        { "machine": "xtool-m2", "laser": "diode-20w", "operation": "engrave",
          "power": 25, "speed": 200, "passes": 1, "line_interval": 0.1, "bidirectional": true },
        { "machine": "xtool-m2", "laser": "diode-20w", "operation": "engrave", "name": "Deep",
          "power": 60, "speed": 150, "passes": 2, "line_interval": 0.1 }
      ]
    }
  ]
}
```

The numbers above are an example of the format, not tested settings.

### Material fields

| Field | Meaning |
|---|---|
| `id` | Unique id, lowercase with dashes. Settings and documents refer to it |
| `name` | Shown in the app |
| `category` | Kind, for sorting and filtering: `paper`, `wood`, `plywood`, `acrylic`, `leather`, `fabric`, `metal`, `stone`, `glass` or `other`. Missing = `other` |
| `thickness` | In mm |
| `color` | Optional `[r, g, b]` (0–255): the sheet's colour in the laser preview. Missing = wood colour |
| `notes` | Anything useful |
| `source` | Where the numbers come from |
| `settings` | The laser settings, below |

### Settings

Each setting is for one **machine**, one **laser module** of that machine and one **operation**:

| Field | Meaning |
|---|---|
| `machine` | The machine's `id`, for example `xtool-m2` (see the [built-in machines](#built-in-machine-ids)) |
| `laser` | The laser module's `id` within that machine, for example `diode-20w` |
| `operation` | `cut`, `score` or `engrave` (older files may say `fill`; it reads as `engrave`) |
| `name` | Optional profile name, for example `"Light"`, `"Deep"`, `"Photo"`. Leave it out for the default setting. A material can have several profiles per machine, laser and operation; layers pick one |
| `power` | Percent, 0–100 |
| `speed` | mm/s |
| `passes` | How many times; missing = 1 |
| `line_interval` | Engrave only: distance between engraved lines in **mm** |
| `bidirectional` | Engrave only: `true` to engrave in both directions |

### Lines per cm and `line_interval`

The app shows the engraving line spacing as **lines per cm**; the file stores the distance between
lines in mm. Convert with *lines per cm = 10 / line_interval*:

| Lines per cm | `line_interval` (mm) |
|---|---|
| 50 | 0.2 |
| 100 | 0.1 |
| 125 | 0.08 |
| 200 | 0.05 |

xTool Studio calls the same number *density* (lines per cm).

## Machines

Settings must point at a machine and a laser module. Scriber has built-in profiles for many xTool
machines; for other machines, add one in the app (Machines page) or in the file's `machines` list:

```json
{
  "id": "my-co2",
  "name": "Workshop CO2 laser",
  "vendor": "Generic",
  "model": "K40",
  "width": 300,
  "height": 200,
  "lasers": [
    { "id": "co2-40w", "name": "40 W CO2 tube", "kind": "co2", "power_w": 40,
      "ops": ["cut", "score", "engrave"] }
  ],
  "notes": "",
  "source": "Measured the bed myself."
}
```

| Field | Meaning |
|---|---|
| `id`, `name`, `vendor`, `model` | Identification |
| `width`, `height` | Work area in mm |
| `lasers` | One entry per laser module: `id`, `name`, `kind` (`diode`, `co2`, `fiber` or `infrared`), `power_w` (watts), `ops` (what it can do: `cut`, `score`, `engrave`), optional `area` `[w, h]` when the module reaches less than the machine |
| `notes`, `source` | Free text; `source` should say where every number comes from |
| `xtool` | xTool machines only: `{"device": "<device code>", "name": "<xTool Studio's name>"}`. Needed for the [.xs export](../guide/exporting#xtool-studio-projects) and the xTool import |
| `xtool_laser` | Per laser, xTool machines only: the module name xTool Studio uses, for example `laser_blue_10W` |

If you change a built-in machine in the app, your copy is stored in your library and wins over the
built-in one.

## Built-in machine ids

| Machine id | Machine | Laser ids |
|---|---|---|
| `xtool-d1` | xTool D1 | `diode-5w`, `diode-10w`, `diode-20w`, `ir-2w` |
| `xtool-d1-pro` | xTool D1 Pro | `diode-5w`, `diode-10w`, `diode-20w`, `diode-40w`, `ir-2w` |
| `xtool-d1-pro-2` | xTool D1 Pro 2.0 | `diode-5w`, `diode-10w`, `diode-20w`, `diode-40w`, `ir-2w` |
| `xtool-f1` | xTool F1 | `diode-10w`, `ir-2w` |
| `xtool-f1-lite` | xTool F1 Lite | `diode-10w` |
| `xtool-f1-ultra` | xTool F1 Ultra | `fiber-20w`, `diode-20w` |
| `xtool-f1-ultra-gs003` | xTool F1 Ultra (GS003) | `fiber-20w`, `diode-20w` |
| `xtool-f2` | xTool F2 | `diode-15w`, `ir-5w` |
| `xtool-f2-ultra` | xTool F2 Ultra | `fiber-60w`, `diode-40w` |
| `xtool-f2-ultra-single` | xTool F2 Ultra (Single) | `fiber-60w` |
| `xtool-m1` | xTool M1 | `diode-10w` |
| `xtool-m1-ultra` | xTool M1 Ultra | `diode-5w`, `diode-10w`, `diode-20w` |
| `xtool-m2` | xTool M2 | `diode-10w`, `diode-20w`, `diode-40w`, `ir-3w` |
| `xtool-metalfab` | xTool MetalFab CNC Cutter | `fiber-1200w`, `fiber-800w` |
| `xtool-p1` | xTool P1 | `co2-40w` |
| `xtool-p2` | xTool P2 | `co2-55w` |
| `xtool-p2s` | xTool P2S | `co2-55w` |
| `xtool-p3` | xTool P3 | `co2-80w`, `ir-5w` |
| `xtool-retail-marker` | xTool Retail Marker | `fiber-20w`, `diode-20w` |
| `xtool-s1` | xTool S1 | `diode-10w`, `diode-20w`, `diode-40w`, `ir-2w`, `ir-3w` |

The machine files live in the repository's
[`machines/`](https://github.com/ExeQue/scriber/tree/main/machines) folder; each file's `source`
says where its numbers come from.

## Next

- [Share a library](./share-a-library) with other Scriber users.
- [Library repositories](/library-repositories): subscribe to libraries others publish.
