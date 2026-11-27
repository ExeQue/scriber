# Contributing to Scriber

Thanks for helping. Bug reports, ideas and pull requests are welcome on GitHub.

## Build and test

You need [Rust](https://rustup.rs) (edition 2024). For the docs, Node.js 18 or newer.

```sh
cargo build                 # debug build: target/debug/scriber
cargo test                  # all tests; they open no window and need no internet
cargo clippy --all-targets   # lints; add no new warnings (a few old ones remain)
npm run docs:install        # once
npm run docs:build          # the docs site; fails on dead links
```

Tests run with a temporary config folder, so they never touch your own library or settings. To try
the MCP tools without a window, run `target/debug/scriber mcp --headless` with
`SCRIBER_CONFIG_DIR` pointing at a scratch folder. More on the code layout in [AGENTS.md](AGENTS.md).

## Code style

- Format new code with `rustfmt`. The existing code is not fully rustfmt-formatted yet, so do not
  reformat files or lines you are not otherwise changing: keep the diff to your change.
- Match the style of the surrounding code: short doc comments that say what a thing is and why,
  plain names, no dead code.
- Units are millimetres, y grows down and angles are clockwise degrees everywhere.

## Pull requests

- Keep them small and focused: one change per pull request.
- Add tests for new behaviour and for bugs you fix.
- Update the docs in `docs/` when behaviour changes.
- When you add or change an MCP tool, update `skills/scriber/SKILL.md` and `docs/agents/`, and
  regenerate the tool reference with `cargo build && npm run docs:mcp`.
- Never commit secrets, tokens, personal files or files with personal data (for example your own
  projects or library files as fixtures).

## Commit messages

Every commit follows [Conventional Commits](https://www.conventionalcommits.org):

```
<type>(<scope>): <description>
```

- Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `ci`, `build`, `perf`, `style`,
  `revert`. The scope is optional and names the area, for example `mcp`, `gui`, `export`.
- The description is imperative and lowercase, with no trailing period:
  `fix(export): keep layer colours distinct in .xs files`.
- Put details in the body, after a blank line.
- Breaking changes get a `!` after the type or scope and a `BREAKING CHANGE:` footer.

## License

By contributing you agree that your contributions are licensed under the [MIT License](LICENSE).
