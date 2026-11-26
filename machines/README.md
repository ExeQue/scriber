# Built-in machines

Each `*.json` file here is one laser cutter that ships with Scriber. The files are built into the app
(`build.rs` picks up every file in this folder), so adding a machine means adding a file and rebuilding.
The file name must be the machine's `id` plus `.json`.

```json
{
  "id": "xtool-m2",
  "name": "xTool M2",
  "vendor": "xTool",
  "model": "M2",
  "width": 426,
  "height": 320,
  "lasers": [
    { "id": "diode-40w", "name": "40 W blue laser", "kind": "diode", "power_w": 40,
      "ops": ["cut", "score", "engrave"], "area": [426, 312] }
  ],
  "notes": "Anything useful for the user.",
  "source": "Where every number comes from (URL, date read), and which values are unverified."
}
```

- `width` × `height`: the work area in mm. New documents can use it as the material size.
- `lasers`: one entry per laser module. `kind` is `diode`, `co2`, `fiber` or `infrared`; `power_w` is the
  laser power in watts; `ops` lists what the module can do (`cut`, `score`, `engrave` = engrave areas;
  open shapes set to engrave are scored); `area` is only needed when the module's work area differs from the machine's.
- `source` is required in practice: only put in numbers you can cite, and say which ones are unverified.
- `xtool` (xTool machines only): `{"device": "<device code>", "name": "<xTool Studio's name>"}`, and per laser
  `xtool_laser`: the module string xTool Studio writes into `.xs` projects (e.g. `laser_blue_10W`), where known.

## xTool machines

| File | Machine | xTool device code | Lasers |
|------|---------|-------------------|--------|
| `xtool-d1` | D1 | MD1 | 5 / 10 / 20 W blue, 2 W infrared |
| `xtool-d1-pro` | D1 Pro | MDP | 5 / 10 / 20 / 40 W blue, 2 W infrared |
| `xtool-d1-pro-2` | D1 Pro 2.0 | MDP2 | 5 / 10 / 20 / 40 W blue, 2 W infrared |
| `xtool-s1` | S1 | MD2 | 10 / 20 / 40 W blue, 2 / 3 W infrared |
| `xtool-m1` | M1 | MLM | 10 W blue |
| `xtool-m1-ultra` | M1 Ultra | ZH009 | 5 / 10 / 20 W blue |
| `xtool-m2` | M2 | JS002 | 10 / 20 / 40 W blue, 3 W infrared |
| `xtool-f1` | F1 | MF1 | 10 W blue, 2 W infrared |
| `xtool-f1-lite` | F1 Lite | GS005 | 10 W blue |
| `xtool-f1-ultra` | F1 Ultra | GS002 | 20 W fiber, 20 W blue |
| `xtool-f1-ultra-gs003` | F1 Ultra (newer extension) | GS003 | 20 W fiber, 20 W blue |
| `xtool-retail-marker` | Retail Marker | GS008 | 20 W fiber, 20 W blue |
| `xtool-f2` | F2 | GS006 | 15 W blue, 5 W infrared |
| `xtool-f2-ultra` | F2 Ultra | GS004-CLASS-4 | 60 W MOPA fiber, 40 W blue |
| `xtool-f2-ultra-single` | F2 Ultra (Single) | GS007-CLASS-4 | 60 W MOPA fiber |
| `xtool-p1` | P1 | MLR | 40 W CO2 |
| `xtool-p2` | P2 | MP2 | 55 W CO2 |
| `xtool-p2s` | P2S | ZY013 | 55 W CO2 |
| `xtool-p3` | P3 | ZY015 | 80 W CO2, 5 W infrared |
| `xtool-metalfab` | MetalFab CNC Cutter | HJ003 | 1200 / 800 W fiber |

These come from the device extensions of xTool Studio 1.9.11 (`~/.config/xTool Studio/atomm/exts/`; the index
`ext.json` lists every device with its code, name and machine type). Per machine, the extension's `index.js` holds a
config object with `deviceCode`, `defaultPower` (module watts) and `processingArea` (work area per processing mode,
for some machines also per module or positioning mode); `xtool-*.json` uses the `LASER_PLANE` area. Wavelengths come from
labels and maps in the extension (`455`, `1064`, `MOPA IR`, `CO2`) where present; each file's `source` says what was
found and what is assumed. Skipped: the printers (DT001, DT002, DT003; machine type PRINT) and the F2 Ultra UV
(GS009-CLASS-1, GS009-CLASS-4; a 5 W UV laser, which Scriber has no laser kind for).

Scriber ships machines only, no material presets: people build their own material library in the app.
The material file format (same as in `library.json`, and used by the test fixtures in
`src/library/test_materials/`):
`name`, `category` (paper, wood, plywood, acrylic, leather, fabric, metal, stone, glass or other; missing = other), `thickness` (mm), `notes`, `source`, and `settings`, each with `machine`, `laser`, `operation`,
`power` (%), `speed` (mm/s), `passes`, and for `engrave` optionally `line_interval` (mm between lines; the app shows it as lines per cm = 10 / `line_interval`, so 0.1 mm = 100 lines/cm) and `bidirectional`.
Older files may say `fill`; it reads as `engrave`.

Users can change or delete built-in machines in the app (Laser > Machines & materials); their copy is stored in
`<config>/scriber/library.json` and wins over the file here with the same id. `cargo test` checks that
every file parses, has a positive work area and that the test materials' settings point at a known machine and laser.
