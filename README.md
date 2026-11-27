# Scriber

2D CAD for laser cutters, made for AI agents and the people who steer them.

Scriber is AI-first: its MCP server lets an AI agent (Claude Code, Codex, Cursor, …) draw, edit, check
and prepare designs for the laser, live in your Scriber window. You can step in at any time: pin a
review comment on the canvas, edit by hand, undo, or take over. The window and the agents work
through the same commands and share one undo history.

The controls follow AutoCAD and DraftSight (command line, dynamic input, constraints, grips,
OSNAP/POLAR), so users of those feel at home quickly.

![Scriber's main window with a demo project](docs/public/screenshots/theme-light.webp)

## Features

- **MCP server for agents**: streamable HTTP on `127.0.0.1:47821` while the editor runs, plus a stdio
  bridge (`scriber mcp`) for agents that only speak stdio. One click (or `scriber agents add`) connects
  the agents it finds and installs the Scriber agent skill where the agent supports skills.
- **Precise 2D CAD**: lines, polylines, arcs, curves, rectangles, polygons, text; fillet, chamfer,
  offset, boolean, arrays, trim, extend, stretch; geometric and dimensional constraints with named
  parameters; snaps, polar tracking and grips.
- **Layers for the laser**: cut, score, engrave or no output, each with its own power, speed, passes
  and line density.
- **Machines and materials**: a library of laser settings per machine and module. Scriber ships
  machine definitions only, no material presets. Subscribe to libraries in GitHub repositories,
  exchange them as `.scrlib`/zip files, or import materials from the xTool Studio cache.
- **Import and export**: SVG, DXF, PNG and xTool Studio `.xs` export; `.xs`, DXF, SVG and image import.
- **Preview and check**: a laser preview and a pre-flight check that catches problems before you cut.
- **Review comments**: annotations on the canvas that agents read, answer and resolve.
- **Several documents** in tabs, and **themes**, including one that follows the
  [Omarchy](https://omarchy.org) desktop theme.

## Install

Scriber runs on Linux (x86_64), macOS (Apple Silicon and Intel) and Windows (x64).

```sh
# Linux and macOS
curl -fsSL https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.ps1 | iex
```

Pinning a version, downloading by hand from the
[Releases page](https://github.com/ExeQue/scriber/releases), building from source and uninstalling
are in the [installation guide](https://exeque.github.io/scriber/guide/installation).

```sh
scriber                  # the editor
scriber box.scrbr        # open a project
scriber mcp              # MCP on stdio (starts the editor if it is not running)
scriber --help           # all commands
```

### Build from source

With [Rust](https://rustup.rs) 1.95 or newer; the
[installation guide](https://exeque.github.io/scriber/guide/installation#build-from-source) lists
the system packages it needs.

```sh
git clone https://github.com/ExeQue/scriber.git
cd scriber
make install                  # ~/.local/bin/scriber and a launcher entry; make uninstall removes them
cargo install --path . --locked   # without make (also Windows): ~/.cargo/bin/scriber
```

## Connecting an agent

In the editor, **AI agents → Connect AI agents…**, or from a terminal:

```sh
scriber agents add
```

Then restart the agent and ask it to draw something. See
[Connecting agents](https://exeque.github.io/scriber/agents/connecting) and
[How agents work with Scriber](https://exeque.github.io/scriber/agents/working-with-agents).

## Documentation

The user guide is online at <https://exeque.github.io/scriber/>. It is a [VitePress](https://vitepress.dev) site in [`docs/`](docs/); to read it locally
(Node.js 18 or newer):

```sh
npm run docs:install                  # once
npm --prefix docs run docs:dev        # http://127.0.0.1:5191
```

Start with [Getting started](https://exeque.github.io/scriber/guide/getting-started).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). AI coding agents: see [AGENTS.md](AGENTS.md).

## Not affiliated with xTool

Scriber is an independent project. It is not affiliated with, endorsed by or sponsored by xTool.
xTool and xTool Studio are trademarks of their respective owners. Scriber's `.xs` support is based on
interoperability analysis of project files that users saved themselves; see the
[.xs format notes](https://exeque.github.io/scriber/xs-format).

## License

[MIT](LICENSE), copyright (c) 2026 Morten Harders. The bundled [Lucide](https://lucide.dev) icon font
is under the ISC license ([`assets/fonts/LUCIDE-LICENSE`](assets/fonts/LUCIDE-LICENSE)).
