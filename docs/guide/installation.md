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
| `SCRIBER_VERSION` | Install this release, e.g. `v0.1.0`, instead of the latest. |
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

You need [Rust](https://rustup.rs) (edition 2024, so Rust 1.85 or newer). On Linux, install the
headers the window toolkit builds against first; on Debian and Ubuntu:

```sh
sudo apt-get install pkg-config libfontconfig1-dev libxkbcommon-dev libwayland-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libgl1-mesa-dev
```

Then:

```sh
git clone https://github.com/ExeQue/scriber.git
cd scriber
cargo build --release
./target/release/scriber              # opens the editor
```

Copy `target/release/scriber` to a folder on your `PATH` to run it as `scriber`.

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
