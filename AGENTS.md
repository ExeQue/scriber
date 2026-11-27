# AGENTS.md

Notes for AI coding agents working on Scriber's code. User-facing docs are in `docs/`; how agents
*use* Scriber over MCP is in `skills/scriber/SKILL.md`.

## What it is

One Rust 2024 binary (`src/main.rs`): the egui/eframe editor (`scriber [FILE]`), the MCP stdio
bridge (`scriber mcp`), a GUI-less MCP server (`scriber mcp --headless`) and agent setup
(`scriber agents`). While the editor runs it serves MCP over streamable HTTP on
`http://127.0.0.1:47821/mcp` (`SCRIBER_PORT` changes the port).

## Map

| Path | What lives there |
|---|---|
| `src/model/` | The document and every command that changes it. GUI and MCP both go through here and share one undo history. |
| `src/model/workspace.rs` | The open documents (tabs), with ids stable for the session, and the active one. |
| `src/model/geom.rs` | Vertices with bulges, contours, transforms, bounding boxes. |
| `src/model/ops/` | CAD operations (fillet, offset, boolean, trim, …) as pure functions on contours. |
| `src/model/constraints/` | Geometric and dimensional constraints, parameter expressions, the Gauss-Newton solver (`solve.rs`); `doc.rs` has `settle`, run after every change. |
| `src/model/annotations.rs` | Review comments: saved in `.scrbr`, never lasered, kept out of undo. |
| `src/model/preflight.rs` | The pre-flight check (pure, changes nothing). |
| `src/model/file.rs` | The `.scrbr` file: a zip with `document.json` and `images/<id>.png`. |
| `src/gui/` | The window: canvas, ribbon (`ribbon/`), command line and drawing commands (`command/`), snaps, grips, panels, themes (`theme/`), library and settings windows. |
| `src/mcp/` | `http.rs` (the editor's endpoint), `relay.rs` (the stdio bridge), `tools.rs` + `tools/` (one tool per document command), `constraint_tools.rs`. |
| `src/export/` | SVG, PNG, DXF (`dxf/`, import too), xTool `.xs` export (`xs/`) and import (`xs_import/`), SVG import (`svg_import/`), images. |
| `src/library/` | Machines and materials: built-ins, the user's `library.json` (`store.rs`), GitHub library sources (`sources.rs`), `.scrlib` exchange, xTool Studio cache import (`xtool/`). |
| `src/agents.rs` | Detects AI agents and adds or removes Scriber's MCP entry and skill. |
| `machines/` | Built-in machine definitions, embedded by `build.rs`. Scriber ships no material presets. |
| `src/library/test_materials/` | Material fixtures, used by tests only. |
| `docs/public/logo.svg`, `assets/scriber.ico` | The logo: the source SVG (docs favicon, window icon, Linux launcher icon) and the Windows program icon made from it (`build.rs` embeds it). |
| `themes/`, `contrib/omarchy/` | Built-in themes and the Omarchy theme template. |
| `skills/scriber/SKILL.md` | The agent skill, embedded in the binary and installed by `scriber agents add`. |
| `docs/` | VitePress user docs; `docs/agents/mcp-tools.md` is generated. |

## Invariants

- Units are millimetres. Origin is the top-left of the material, x grows right, **y grows down**
  (as in SVG). Angles are degrees, positive = **clockwise** on screen.
- A contour is a polyline whose segments may be arcs: each vertex has `bulge` = tan(sweep / 4) for
  the segment to the next vertex (0 = straight).
- Every change to a document goes through `Document` commands in `src/model`, so the GUI and MCP
  stay equivalent and share undo. After a change the constraint solver (`settle`) must keep all
  constraints satisfied; a change it cannot satisfy is refused and undone.
- MCP tools work on one document: `Server::for_call` (`src/mcp/tools/documents.rs`) picks the
  document from the optional `document` argument, else the active tab. New document tools get this
  for free; do not reach for the active document directly.
- Lock order: the workspace lock is never held while a document is locked (see `workspace.rs`).

## Build and test

```sh
cargo build
cargo test
```

The `Makefile` wraps these (`make help` lists the targets): `make build`, `make test`, `make lint`,
`make docs-build`, `make mcp-docs`, `make headless` (on a throwaway config folder) and `make check`
(tests + docs build). Pass cargo flags with `CARGO_FLAGS="-j 4"`. Never use `make run` or
`make install*` from an agent: `run` opens an editor, `install` overwrites the user's `scriber`.

- Unit tests sit next to the code (`#[cfg(test)]` modules and `tests.rs` files). MCP tools are
  tested in `src/mcp/tools/tests.rs` against an in-memory `Workspace`, without a window.
- Under `cfg(test)` the config folder is a temp folder and GUI settings are defaults, so tests never
  touch the user's library or settings. Keep it that way: no test may read or write the real
  `<config>/scriber/`.
- To drive the MCP tools by hand, use the headless server; it opens no window:
  `target/debug/scriber mcp --headless`. Point it at a throwaway config:
  `SCRIBER_CONFIG_DIR=$(mktemp -d)` (`settings.json`, `library.json`, `sources.json` and `themes/` follow it; `SCRIBER_LIBRARY`
  moves `library.json` alone, `SCRIBER_XTOOL_DIR` the xTool Studio folder that is read).
- Library sources are fetched through the `Http` trait; tests use a fake. No network in tests.
- Docs: `npm run docs:install` once, then `npm --prefix docs run docs:build`, which must end with
  "build complete" (it fails on dead links).

## Rules

- **Never kill, restart or launch the user's running Scriber window.** `cargo run`, `scriber` and
  `scriber mcp` without `--headless` all start or attach to the editor; use `--headless` or tests.
- Never write to the user's real library or config. Use `SCRIBER_CONFIG_DIR` / `SCRIBER_LIBRARY`
  with a temp folder.
- When you add or change an MCP tool, update `skills/scriber/SKILL.md` and the docs in
  `docs/agents/`, and regenerate the tool reference: `cargo build && npm run docs:mcp`.
- When behaviour changes, update the matching page in `docs/`.
- Commits follow Conventional Commits, see [CONTRIBUTING.md](CONTRIBUTING.md).
- `SPEC.md` is a local, gitignored planning file. Never commit it or copy from it into the repo.
- Never commit secrets, tokens or personal files.
