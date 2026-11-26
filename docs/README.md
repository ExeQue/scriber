# Scriber docs

The user documentation, built with [VitePress](https://vitepress.dev). Pages are Markdown files in
this folder; the sidebar is in `.vitepress/config.mts`.

## Run it locally

Needs Node.js 18 or newer. From the repository root:

```sh
npm run docs:install   # once: installs VitePress into docs/node_modules
npm run docs           # dev server with live reload at http://127.0.0.1:5191
npm run docs:build     # static site in docs/.vitepress/dist (fails on dead links)
npm run docs:preview   # serve the built site at http://127.0.0.1:5191
```

The same scripts exist inside `docs/` as `docs:dev`, `docs:build` and `docs:preview`. The dev server
takes VitePress flags after `--`, for example
`npm --prefix docs run docs:dev -- --host 0.0.0.0 --port 5191 --strictPort`; a `--host` or `--port`
given there replaces the default (127.0.0.1:5191) set in `.vitepress/config.mts`.

### Through lerd

With the repository's `.lerd.yaml`, run `lerd link --yes` once in the repository root; the docs are
then served at https://scriber-docs.test.

## The MCP tool reference

`agents/mcp-tools.md` is generated from the tool list of `scriber mcp --headless`. After changing
MCP tools, build Scriber and regenerate it:

```sh
cargo build
npm run docs:mcp       # or: npm --prefix docs run docs:mcp -- path/to/scriber
```
