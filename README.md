# SnapSync

[![CI](https://github.com/doutorinfamous/snapsync/actions/workflows/ci.yml/badge.svg)](https://github.com/doutorinfamous/snapsync/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/doutorinfamous/snapsync)](https://github.com/doutorinfamous/snapsync/releases/latest)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

SnapSync is an open-source desktop app that finds a
[Snapmaker U1](https://www.snapmaker.com/en/snapmaker-u1) on your local network
and automatically copies its timelapse videos to a folder on Windows or macOS.

> SnapSync is a community project. It is not affiliated with, endorsed by, or
> supported by Snapmaker.

## Download

Download the latest installer from
**[GitHub Releases](https://github.com/doutorinfamous/snapsync/releases/latest)**:

- **Windows:** `SnapSync_*_x64-setup.exe`
- **macOS:** `SnapSync_*_universal.dmg` for Intel and Apple Silicon Macs

The first releases are not code-signed. Windows SmartScreen and macOS Gatekeeper
may therefore show a warning even when the file was downloaded from this
repository. On macOS, use **System Settings → Privacy & Security → Open Anyway**
after the first blocked launch. Code signing and Apple notarization are planned.

## Features

- Finds Snapmaker U1 printers over mDNS (`_snapmaker._tcp.local.`)
- Supports a direct IP address when discovery is unavailable
- Reads timelapses through the printer's local Moonraker HTTP API
- Uses atomic downloads (`.part`, size validation, then rename)
- Keeps persistent deduplication data and recent sync history
- Supports configurable background schedules and launch at sign-in
- Lives in the system tray while syncing in the background
- Optionally saves JPG thumbnails next to videos
- Never deletes timelapses from the printer

## Screenshot

> A current application screenshot will be added before the first stable
> release.

## Requirements

- A Snapmaker U1 and the computer running SnapSync on the same local network
- Windows 10 or later, or macOS 10.15 or later
- Local firewall access to:
  - UDP 5353 for mDNS discovery
  - TCP 7125 for Moonraker
  - TCP 8080 for the U1 download fallback

## Getting started

1. Install and open SnapSync.
2. Select **Settings → Printer**.
3. Choose **Search network**, or enter the U1 IP address manually.
4. Select **Test** to verify the local HTTP connection.
5. Under **General**, choose a destination folder and save the settings.
6. Select **Sync now** or enable the automatic schedule.

SnapSync queries `/server/files/list?root=camera`, downloads each new video, and
checks its size before moving it into the destination folder. Already downloaded
files are skipped while their local copy remains valid.

## Technology stack

- [Vue 3](https://vuejs.org/) and TypeScript for the interface
- [Vite](https://vite.dev/) for frontend development and builds
- [Tauri 2](https://v2.tauri.app/) for the native desktop shell and installers
- [Rust](https://www.rust-lang.org/) and [Tokio](https://tokio.rs/) for discovery,
  scheduling, file transfer, and local persistence
- mDNS for printer discovery
- HTTP and the [Moonraker API](https://moonraker.readthedocs.io/) for local
  timelapse access

## Development

Prerequisites:

- Node.js 20 or newer
- Stable Rust
- Tauri 2 system dependencies
- Windows: Visual Studio Build Tools with C++ and WebView2
- macOS: Xcode Command Line Tools

Install dependencies and start the development app:

```sh
npm ci
npm run tauri dev
```

Run all checks:

```sh
npm run check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Create a native installer on the current operating system:

```sh
npm run tauri build
```

Windows builds the NSIS `.exe` on a Windows machine. macOS builds the `.dmg` on
a Mac. GitHub Actions performs both builds for tagged releases.

## Releasing

Releases are built from version tags on the `main` branch.

1. Set the same `X.Y.Z` version in:
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
2. Merge the version change into `main`.
3. Create and push the matching tag:

```sh
git tag vX.Y.Z
git push origin vX.Y.Z
```

The release workflow verifies the versions, runs the tests, builds the Windows
NSIS installer and universal macOS DMG, and publishes both files under
[GitHub Releases](https://github.com/doutorinfamous/snapsync/releases).

## Network, privacy, and security

SnapSync communicates directly with the printer over the local network. It does
not use a cloud relay, send analytics, or upload timelapses to a third party.
Logs contain operational messages but not video contents or credentials.

The U1 camera process that defines some local video URLs is proprietary and is
not part of this repository. SnapSync validates returned URLs so downloads stay
on the configured printer host.

## Contributing

Issues and focused pull requests are welcome. Before opening a pull request, run:

```sh
npm run check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## Developed by

SnapSync is developed and maintained by
**[Papai Nerd](https://github.com/doutorinfamous)**.

Model and project references:

- [Snapmaker U1 official product page](https://www.snapmaker.com/en/snapmaker-u1)
- [Snapmaker U1 Wiki](https://wiki.snapmaker.com/en/snapmaker_u1)
- [Moonraker](https://github.com/Arksine/moonraker)
- [Tauri](https://github.com/tauri-apps/tauri)

## License

SnapSync is free and open-source software licensed under the
[GNU General Public License v3.0](LICENSE).
