# Shortcuts for the commands in CONTRIBUTING.md and AGENTS.md. Only `run` opens the editor window.

CARGO_FLAGS ?=
# Arguments for `make run`, e.g. make run ARGS=box.scrbr
ARGS ?=
# Where `make install` puts things. Packagers: make install PREFIX=/usr DATADIR=/usr/share DESTDIR=pkg
PREFIX ?= $(HOME)/.local
DATADIR ?= $(or $(XDG_DATA_HOME),$(HOME)/.local/share)
DESTDIR ?=
# DEV=1 installs the debug build instead of the release build.
DEV ?=

ifeq ($(DEV),)
PROFILE_FLAG := --release
PROFILE_DIR := release
else
PROFILE_FLAG :=
PROFILE_DIR := debug
endif

BIN := $(DESTDIR)$(PREFIX)/bin/scriber
DESKTOP := $(DESTDIR)$(DATADIR)/applications/scriber.desktop
ICON := $(DESTDIR)$(DATADIR)/icons/hicolor/scalable/apps/scriber.svg

.PHONY: help dev build release run test lint check docs-install docs docs-build mcp-docs headless \
	install install-dev uninstall

help: ## List the targets
	@grep -E '^[a-z -]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "} {printf "  %-13s %s\n", $$1, $$2}'

dev build: ## Debug build: target/debug/scriber
	cargo build $(CARGO_FLAGS)

release: ## Release build: target/release/scriber
	cargo build --release $(CARGO_FLAGS)

run: ## Debug build, then open the editor (ARGS=... for its arguments)
	cargo run $(CARGO_FLAGS) -- $(ARGS)

test: ## Run all tests (no window, no network)
	cargo test $(CARGO_FLAGS)

lint: ## Clippy lints; add no new warnings (CI does not run it)
	cargo clippy --all-targets $(CARGO_FLAGS)

check: test docs-build ## What must pass before a pull request

docs-install: ## Install the docs dependencies (once)
	npm run docs:install

docs: ## Docs dev server
	npm run docs

docs-build: ## Build the docs site; fails on dead links
	npm run docs:build

mcp-docs: build ## Regenerate docs/agents/mcp-tools.md
	npm run docs:mcp

headless: build ## MCP server without a window, on a throwaway config folder
	SCRIBER_CONFIG_DIR="$$(mktemp -d)" target/debug/scriber mcp --headless

install: ## Build and install to PREFIX/bin (~/.local/bin), plus a launcher entry on Linux
	cargo build $(PROFILE_FLAG) --locked $(CARGO_FLAGS)
	mkdir -p "$(DESTDIR)$(PREFIX)/bin"
	cp target/$(PROFILE_DIR)/scriber "$(BIN).tmp" && chmod 755 "$(BIN).tmp" && mv -f "$(BIN).tmp" "$(BIN)"
	@if [ "$$(uname -s)" = Linux ]; then \
		mkdir -p "$(DESTDIR)$(DATADIR)/applications" "$(dir $(ICON))" && \
		cp docs/public/logo.svg "$(ICON)" && \
		printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Scriber' 'GenericName=Laser CAD' \
			'Comment=2D CAD for laser cutters, made for AI agents' 'Exec="$(PREFIX)/bin/scriber" %f' \
			'Icon=scriber' 'Terminal=false' 'Categories=Graphics;VectorGraphics;Engineering;' 'StartupWMClass=scriber' \
			> "$(DESKTOP)" && echo "Installed $(DESKTOP)"; \
	fi
	@echo "Installed $(BIN)"

install-dev: ## Install the debug build the same way (to try a branch as `scriber`)
	$(MAKE) install DEV=1

uninstall: ## Remove what install put there; settings, projects and library stay
	rm -f "$(BIN)" "$(DESKTOP)" "$(ICON)"
