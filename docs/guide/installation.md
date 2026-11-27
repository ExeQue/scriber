# Installation

Scriber is one program file. Fonts, icons and machine definitions are built in, so there is nothing
else to install. Prebuilt versions exist for:

| System | Build |
| --- | --- |
| Linux, x86_64 | `x86_64-unknown-linux-gnu` |
| macOS, Apple Silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Windows, x64 (also runs on Windows 11 on Arm) | `x86_64-pc-windows-msvc` |

On other systems, such as Linux on Arm, [build from source](#build-from-source).

## Install with one command

**Linux and macOS**, in a terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.sh | sh
```

This downloads the latest release, checks its SHA-256 checksum and puts `scriber` in
`~/.local/bin`. On Linux it also adds Scriber to your application launcher. If `~/.local/bin` is not
on your `PATH`, the script says so and shows the line to add to your shell profile.

**Windows**, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.ps1 | iex
```

This downloads the latest release, checks its checksum, installs it in
`%LOCALAPPDATA%\Programs\Scriber`, adds that folder to your user `PATH` and makes a Start Menu
shortcut. Open a new terminal afterwards to use `scriber` there.

To **update**, run the same command again. Close Scriber first, or restart it afterwards.

The scripts are in the repository's [`install/`](https://github.com/ExeQue/scriber/tree/main/install)
folder if you want to read them before you run them.

### Options

Both scripts read these environment variables:

| Variable | Meaning |
| --- | --- |
| `SCRIBER_VERSION` | Install this release, e.g. `v0.1.0`, instead of the latest; `dev` installs the [development build](#development-builds). |
| `SCRIBER_INSTALL_DIR` | Install here instead of the default folder. |
| `SCRIBER_UNINSTALL` | `1` removes Scriber, see [Uninstall](#uninstall). |
| `SCRIBER_DOWNLOAD_BASE` | Download from `<url>/<tag>/<file>` instead of GitHub Releases, for mirrors and for testing the scripts. |

To install a particular version:

```sh
curl -fsSL https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.sh | SCRIBER_VERSION=v0.1.0 sh
```

```powershell
$env:SCRIBER_VERSION = 'v0.1.0'; irm https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.ps1 | iex
```

### Development builds

Every change on `main` is built for all platforms and published as the
[`dev` prerelease](https://github.com/ExeQue/scriber/releases/tag/dev). It has the newest features
and fixes, but it is **unstable**: it is not tested beyond the automatic tests, it can break your
projects or library, and, like the releases, it is not signed. Save copies of projects you care
about first.

```sh
curl -fsSL https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.sh | SCRIBER_VERSION=dev sh
```

```powershell
$env:SCRIBER_VERSION = 'dev'; irm https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.ps1 | iex
```

Run the same line again to update to the newest development build. To go back to the latest
stable release, run the normal install command from the top of this page.

## Download by hand

Every release on the [Releases page](https://github.com/ExeQue/scriber/releases) has an archive per
system, named `scriber-<version>-<build>.tar.gz` (`.zip` on Windows), and a `.sha256` file next to it.
The archive holds the `scriber` program (`scriber.exe` on Windows), the license and the readme.

1. Download the archive for your system and its `.sha256` file.
2. Check the download. In the folder with both files, the command must print `OK`; on Windows, the
   printed hash must match the one in the `.sha256` file:

   ```sh
   sha256sum -c scriber-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256    # Linux
   shasum -a 256 -c scriber-v0.1.0-aarch64-apple-darwin.tar.gz.sha256    # macOS
   ```

   ```powershell
   Get-FileHash scriber-v0.1.0-x86_64-pc-windows-msvc.zip    # Windows
   ```

3. Unpack it and put `scriber` somewhere on your `PATH`, such as `~/.local/bin`.

## Unsigned program warnings

Scriber is not code-signed yet, so the operating system may warn the first time you start it.

- **macOS** (Gatekeeper): the install script removes the quarantine mark, so this only happens after a
  download by hand. Remove the mark yourself with `xattr -d com.apple.quarantine ~/.local/bin/scriber`,
  or open **System Settings → Privacy & Security** after the first refused start and choose
  **Open Anyway**.
- **Windows** (SmartScreen): if **Windows protected your PC** appears, choose **More info**, then
  **Run anyway**. For a zip downloaded in a browser you can also right-click it, choose
  **Properties** and tick **Unblock** before you unpack it.

## Build from source

### What you need

- **Rust 1.95 or newer** (edition 2024): install [rustup](https://rustup.rs), then
  `rustup toolchain install stable`. `rustup update` brings an older toolchain up to date.
- **Linux**: a C compiler and the headers the window toolkit builds against.
  - Debian and Ubuntu:
    ```sh
    sudo apt-get install build-essential pkg-config libfontconfig1-dev libxkbcommon-dev \
      libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libgl1-mesa-dev
    ```
  - Fedora:
    ```sh
    sudo dnf install gcc make pkgconf-pkg-config fontconfig-devel libxkbcommon-devel \
      wayland-devel libxcb-devel mesa-libGL-devel
    ```
  - Arch:
    ```sh
    sudo pacman -S --needed base-devel fontconfig libxkbcommon wayland libxcb mesa
    ```
- **macOS**: the Xcode command line tools: `xcode-select --install`.
- **Windows**: the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with the **Desktop development with C++** workload (rustup offers to install them).

### Build and install

**Linux and macOS**:

```sh
git clone https://github.com/ExeQue/scriber.git
cd scriber
make install
```

This builds a release binary and puts it in `~/.local/bin/scriber`; on Linux it also adds Scriber to
your application launcher, the same as the install script. `make uninstall` removes both again. Set
`PREFIX` to install elsewhere, for example `make install PREFIX=/usr/local` (with `sudo`); packagers
can also set `DATADIR` and `DESTDIR`. `make help` lists every target.

**Without make** (also on Windows), let Cargo install it in `~/.cargo/bin`, which rustup puts on your
`PATH`:

```sh
git clone https://github.com/ExeQue/scriber.git
cd scriber
cargo install --path . --locked
```

This adds no launcher entry. `cargo uninstall scriber` removes it.

### Working on Scriber

To work on Scriber itself (more in
[CONTRIBUTING.md](https://github.com/ExeQue/scriber/blob/main/CONTRIBUTING.md)):

```sh
make dev        # debug build: target/debug/scriber
make run        # debug build, then open the editor; make run ARGS=box.scrbr opens a project
make test       # all tests; they open no window and need no internet
make headless   # the MCP server without a window, on a throwaway settings folder
make check      # what must pass before a pull request
make install-dev  # install the debug build as ~/.local/bin/scriber, to try a branch for real
```

The debug build is optimised (Scriber at `opt-level = 1`, its dependencies at `opt-level = 3`), so
it is fast enough to draw with while it still builds quickly.

A debug build shares settings, library and MCP port with an installed Scriber. To keep them apart:

- `SCRIBER_CONFIG_DIR=/some/folder` gives it its own settings, library and themes.
- `SCRIBER_PORT=47822` gives it its own MCP port. Only one editor can serve a port: a second editor
  on the same port still opens, but serves no MCP and says so in the status bar, so agents keep
  talking to the first one. Run `scriber mcp` with the same `SCRIBER_PORT` to reach the second.

```sh
SCRIBER_CONFIG_DIR=$(mktemp -d) SCRIBER_PORT=47822 make run
```

## Uninstall

Run the install script with the uninstall option. It removes the program, the launcher entry or
Start Menu shortcut and (on Windows) the `PATH` entry. Your settings, projects and material library
stay where they are.

```sh
curl -fsSL https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.sh | sh -s -- --uninstall
```

```powershell
$env:SCRIBER_UNINSTALL = '1'; irm https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.ps1 | iex
```

If you installed with `SCRIBER_INSTALL_DIR`, set it again when you uninstall.

## Next steps

Connect your AI agents, from the editor (**AI agents → Connect AI agents…**) or a terminal:

```sh
scriber agents add
```

See [Connecting agents](../agents/connecting), then [Getting started](./getting-started).
