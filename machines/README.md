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

Material presets work the same way in `../materials/` (one material per file, `id` = file name):
`name`, `thickness` (mm), `notes`, `source`, and `settings`, each with `machine`, `laser`, `operation`,
`power` (%), `speed` (mm/s), `passes`, and for `engrave` optionally `line_interval` (mm) and `bidirectional`.
Older files may say `fill`; it reads as `engrave`.
Only add settings the vendor publishes, with the page in `source`.

Users can change or delete built-ins in the app (Laser > Machines & materials); their copy is stored in
`<config>/scriber/library.json` and wins over the file here with the same id. `cargo test` checks that
every file parses, has a positive work area and that material settings point at a known machine and laser.
