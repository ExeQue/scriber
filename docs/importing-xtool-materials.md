# Importing materials from xTool Studio

Scriber can copy laser settings (power, speed, passes, lines per cm) from xTool Studio into your
material library. It reads what xTool Studio already keeps on your computer. Nothing is downloaded and
you do not log in to anything from Scriber.

## 1. Get the material into xTool Studio's cache

xTool Studio only keeps the materials you have looked at. For each material you want:

- open it in xTool Studio's material library with your machine selected (for example the M2), or
- on atomm.com's Materials Lab, log in with the same xTool account, find the material and use
  "Save to Studio", then open xTool Studio once.

## 2. Import it in Scriber

- **In the window:** Laser tab → Machines & materials… → **Import ▾ → From xTool Studio…**. Search,
  tick the materials and press Import. Then press Save.
- **With an AI agent:** ask it to find the material; it uses `search_xtool_materials` and
  `import_xtool_materials`.

You can also import the settings stored in an xTool Studio project: **Import ▾ → xTool project (.xs)…**
(agents: `import_xtool_project`). The project does not hold the material's name, so the new material
gets the file's name; rename it afterwards.

## What gets imported

- Cut (xTool "vector cutting"), score ("vector engraving") and engrave ("fill engraving", with its
  density as lines per cm). Image (bitmap) engraving settings are skipped.
- One setting per laser module and operation; when xTool has several variants, its default one.
- Only machines in your library with an xTool device code (all built-in xTool machines). Others are
  listed but cannot be imported.
- The material's `source` says where it came from and when. Importing the same xTool material again
  adds a copy, unless you tick "Replace materials imported … before" (agents: `overwrite: true`).

## Good to know

- Tested with xTool Studio 1.9.11. Its cache format is internal and may change with updates; if the
  import stops working after an xTool update, please report it.
- Scriber copies the cache to a temporary folder, reads only the material catalogue and the material
  settings from the copy, and deletes the copy. It never writes to xTool Studio's folder and never
  reads your account, devices or job history.
- Where Scriber looks: Linux `~/.config/xTool Studio`, Windows `%APPDATA%\xTool Studio`, macOS
  `~/Library/Application Support/xTool Studio` (also `xTool_Studio`), and under WSL the Windows users'
  folders. Set `SCRIBER_XTOOL_DIR` to xTool Studio's folder if it is somewhere else.
- On Windows, if reading fails while xTool Studio is running, close xTool Studio and try again.

## Sharing your library with other Scriber users

**Export ▾** in the same window saves the selected material (with the machines its settings use), the
selected machine, or the whole library to a `.scrlib` file. **Import ▾ → Scriber library file…** merges
one into yours; when an id is already taken by a different machine or material you choose Replace,
Keep both (the import gets a new id) or Skip. Settings for machines you do not have are kept, and the
import says which machines they are. Agents: `export_library` and `import_library`.
