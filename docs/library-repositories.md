# Library repositories

Anyone can publish machines and materials for Scriber in a public GitHub repository, and anyone can
subscribe to it. Scriber keeps subscribed libraries in sync, so fixes and new materials reach
everyone who uses them.

## Subscribing

In **Laser → Machines & materials… → Sources**, paste a repository and press **Subscribe**:

- `owner/repo`: follows the repository's default branch;
- `owner/repo@v1.2`: follows the tag `v1.2` when the repository has that tag, else the branch `v1.2`;
- `https://github.com/owner/repo` or `https://github.com/owner/repo/tree/<branch>`.

Each source follows one thing, which you can change with **Follow ▾**:

| Follow | What a sync does |
|--------|------------------|
| Default branch / a branch | takes the newest commit on it |
| A tag (or a release's tag) | stays on that tag's commit; a sync only notices when the tag is moved |
| Latest release | takes GitHub's latest release; a new release is an update |
| This commit | locked: a sync does nothing |

The machines and materials of a source show in your library with "from &lt;source name&gt;". They are
read-only: **Duplicate** makes your own copy, which you can change. Turn a source off with its tick
box to hide its items without unsubscribing; **✕** unsubscribes (after asking) and deletes its files.

### Syncing

Scriber syncs your sources when it starts, at most once a day per source, in the background (the
window never waits). **Sync now** and **Sync all** sync right away. A sync first asks GitHub for the
commit the source follows (one small request; an unchanged answer does not count against GitHub's
rate limit). Only when that commit differs from the one in use is the repository downloaded. The
Sources tab and the status bar say what changed, e.g. "+2 materials, 1 changed, 0 removed".

A new version is only used when it reads fine; otherwise the one in use stays and the error is shown.
Offline at start? The error is noted and the next start tries again.

### Which one wins

When two have the same id: your own machines and materials (and your changes) win over sources, and
sources win over Scriber's built-in machines. Between two sources the one higher in the list wins;
the Sources tab reports the clash. Settings for machines you do not have are kept and flagged.

### Safety

Scriber downloads the repository as a zip over HTTPS (no git needed) and takes only `.scriber.json`
and the `.json` files under `.scriber/`: at most 50 MB per download, 1 MB per file and 5000 files;
paths that try to leave the folder are refused. Nothing in a repository is ever run. Files that are
not valid machines or materials are skipped and reported. Only public repositories work. If you hit
GitHub's rate limit, set the `GITHUB_TOKEN` environment variable; Scriber sends it to GitHub's API
only and never stores it.

Files are kept in `<config>/scriber/sources/<id>/<commit>/`, the list in `<config>/scriber/sources.json`
(`<config>` is `~/.config` on Linux; `SCRIBER_CONFIG_DIR` moves the whole `scriber` folder).

## Publishing your own

**Export ▾ → Library repository…** writes the machines and materials you made yourself into a folder:
not the built-in ones, not ones from sources, and not your edits of those (duplicate an item first to
make it your own). Materials may use built-in machines; those are referred to by id, not copied.

- A new folder gets a `.scriber.json`; Scriber asks for its name and description.
- An existing repository is updated in place: only `.scriber.json` and `.scriber/machines|materials/`
  are touched. Files there for items you no longer have are removed; everything else (`.git`, README,
  licence, …) stays. The manifest keeps its name, description, author, licence and homepage; its
  `library_version`, when it has one, goes up by one when something changed.

Scriber does no git: commit and push the folder yourself, e.g. `git add -A && git commit -m "Update
library" && git push`. Then tell people your `owner/repo`. Tag a release (`v1.0`) when you want
people to be able to stay on a tested version.

**Export ▾ → Library zip…** writes the same layout into one zip; **Import ▾ → Library zip…** merges
one into your library (an id that is taken asks Replace / Keep both / Skip).

## The format

```text
my-laser-library/
├── .scriber.json
├── .scriber/
│   ├── machines/
│   │   └── my-k40.json
│   └── materials/
│       ├── birch-plywood-3mm.json
│       └── cast-acrylic-3mm.json
└── README.md            (anything else is ignored)
```

`.scriber.json` at the repository root:

```json
{
  "format": "scriber-library",
  "version": 1,
  "name": "My laser library",
  "description": "Tested settings for my K40 and xTool M2",
  "author": "Jane Doe",
  "license": "CC0-1.0",
  "homepage": "https://example.com/laser",
  "library_version": "1.0"
}
```

`format`, `version` and `name` are required; `homepage` and `library_version` are optional.

Every file in `.scriber/machines/` is one machine and every file in `.scriber/materials/` one material,
in Scriber's own format (the same JSON as in `library.json`; machines are described in
`machines/README.md` of Scriber's source). The file name must be the id plus `.json`, and ids use
letters, digits, `-` and `_`. A material's settings name machines by id: a built-in machine
(e.g. `xtool-m2`), one in the same repository, or one the user has.

```json
{
  "id": "birch-plywood-3mm",
  "name": "Birch plywood 3 mm",
  "category": "plywood",
  "thickness": 3,
  "settings": [
    {"machine": "xtool-m2", "laser": "diode-10w", "operation": "cut", "power": 100, "speed": 6, "passes": 2},
    {"machine": "xtool-m2", "laser": "diode-10w", "operation": "engrave", "power": 40, "speed": 200, "line_interval": 0.1}
  ],
  "source": "Own tests, November 2026"
}
```

## For agents (MCP)

`list_library_sources`, `subscribe_library { url, track? }`, `sync_library_sources { id? }`,
`set_library_source { id, track?, enabled? }`, `unsubscribe_library { id }` and
`export_library_repository { dir, machines?, materials?, name?, description? }`. A `track` is
`{"branch": "main"}` (`""` = default branch), `{"tag": "v1.2"}`, `{"release": "latest"}` or
`{"commit": "<sha>"}`. `export_library` and `import_library` take a `.zip` path for library zips.
The headless MCP server (`scriber mcp --headless`) never syncs by itself.
