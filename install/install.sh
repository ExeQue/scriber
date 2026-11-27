#!/bin/sh
# Installs Scriber on Linux (x86_64) and macOS (Apple Silicon and Intel).
#
#   curl -fsSL https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.sh | sh
#
# Environment:
#   SCRIBER_VERSION=v0.1.0      install this release instead of the latest
#                               (dev: the unstable build of the newest commit on main)
#   SCRIBER_INSTALL_DIR=<dir>   where the binary goes (default: ~/.local/bin)
#   SCRIBER_UNINSTALL=1         remove the binary and desktop entry (same as --uninstall)
#   SCRIBER_DOWNLOAD_BASE=<url> download releases from <url>/<tag>/<asset> (mirrors and testing)
#
# Re-running the script upgrades an existing install. Uninstalling never touches your settings,
# projects or material library.
set -eu

REPO=ExeQue/scriber
INSTALL_DIR=${SCRIBER_INSTALL_DIR:-$HOME/.local/bin}
DOWNLOAD_BASE=${SCRIBER_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download}
DESKTOP_FILE=${XDG_DATA_HOME:-$HOME/.local/share}/applications/scriber.desktop
UNINSTALL=${SCRIBER_UNINSTALL:-0}

for arg in "$@"; do
    case $arg in
        --uninstall) UNINSTALL=1 ;;
        *) echo "scriber install: unknown option $arg" >&2; exit 2 ;;
    esac
done

die() {
    echo "scriber install: $*" >&2
    exit 1
}

if [ "$UNINSTALL" = 1 ]; then
    rm -f "$INSTALL_DIR/scriber" "$DESKTOP_FILE"
    echo "Removed $INSTALL_DIR/scriber and $DESKTOP_FILE."
    echo "Your settings, projects and material library are left as they are."
    exit 0
fi

os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
    Linux/x86_64 | Linux/amd64) target=x86_64-unknown-linux-gnu ;;
    Darwin/arm64 | Darwin/aarch64) target=aarch64-apple-darwin ;;
    Darwin/x86_64) target=x86_64-apple-darwin ;;
    *) die "there is no prebuilt Scriber for $os $arch yet (builds exist for Linux x86_64 and macOS). Build it from source: https://exeque.github.io/scriber/guide/installation#build-from-source" ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q "$1" -O "$2"; }
else
    die "needs curl or wget"
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    die "needs sha256sum or shasum to verify the download"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM

tag=${SCRIBER_VERSION:-}
if [ -z "$tag" ]; then
    fetch "https://api.github.com/repos/$REPO/releases/latest" "$tmp/latest.json" ||
        die "could not look up the latest release; set SCRIBER_VERSION (e.g. v0.1.0) to pick one"
    tag=$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1)
    [ -n "$tag" ] || die "could not read the latest release tag from the GitHub API"
fi

asset="scriber-$tag-$target.tar.gz"
echo "Downloading $asset"
fetch "$DOWNLOAD_BASE/$tag/$asset" "$tmp/$asset" || die "download failed: $DOWNLOAD_BASE/$tag/$asset"
fetch "$DOWNLOAD_BASE/$tag/$asset.sha256" "$tmp/$asset.sha256" || die "download failed: $asset.sha256"

expected=$(cut -d' ' -f1 "$tmp/$asset.sha256")
actual=$(sha256 "$tmp/$asset")
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    die "checksum mismatch for $asset (expected $expected, got $actual)"
fi

mkdir -p "$tmp/pkg" "$INSTALL_DIR"
tar -xzf "$tmp/$asset" -C "$tmp/pkg"
[ -f "$tmp/pkg/scriber" ] || die "$asset has no scriber binary"
# Copy next to the target, then rename, so a running Scriber keeps its old binary.
cp "$tmp/pkg/scriber" "$INSTALL_DIR/.scriber.new"
chmod 755 "$INSTALL_DIR/.scriber.new"
mv -f "$INSTALL_DIR/.scriber.new" "$INSTALL_DIR/scriber"

if [ "$os" = Darwin ]; then
    # The binaries are not signed; without this Gatekeeper refuses to start the downloaded one.
    xattr -d com.apple.quarantine "$INSTALL_DIR/scriber" 2>/dev/null || true
else
    mkdir -p "$(dirname "$DESKTOP_FILE")"
    cat >"$DESKTOP_FILE" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Scriber
GenericName=Laser CAD
Comment=2D CAD for laser cutters, made for AI agents
Exec="$INSTALL_DIR/scriber" %f
Terminal=false
Categories=Graphics;VectorGraphics;Engineering;
StartupWMClass=scriber
DESKTOP
fi

echo "Installed Scriber $tag to $INSTALL_DIR/scriber"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Note: $INSTALL_DIR is not on your PATH. Add it in your shell profile, for example:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac
echo "Next: run 'scriber agents add' to connect your AI agents (https://exeque.github.io/scriber/agents/connecting)."
