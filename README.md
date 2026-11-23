<div align="center">

# Modwire

**Publish a new version of your Minecraft mod to Modrinth and CurseForge in one click.**

[![Release](https://img.shields.io/github/v/release/Waffylka/ModWire?color=e8a54b&label=version)](https://github.com/Waffylka/ModWire/releases/latest)
[![CI](https://github.com/Waffylka/ModWire/actions/workflows/ci.yml/badge.svg)](https://github.com/Waffylka/ModWire/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/Windows%20%7C%20macOS-1d1a17)
![Rust](https://img.shields.io/badge/Rust-egui-e8a54b)

[**Download the latest version**](https://github.com/Waffylka/ModWire/releases/latest)

**English** · [Українська](README.uk.md)

</div>

A native desktop app written in Rust ([egui](https://github.com/emilk/egui)). Pick your mod file, check the version, compatibility and changelog, and Modwire uploads it to Modrinth and CurseForge at the same time.

> The app interface is currently in Ukrainian.

## Features

- **One release, two platforms.** Progress, speed and cancel are tracked per platform. If one platform rejects the file, retry just that one without creating a duplicate on the other.
- **Autofill from the file.** Version number, name, exact Minecraft versions and loaders (Fabric, Quilt, Forge, NeoForge) are read from `.jar` and `.mrpack` files.
- **Projects.** Add them from your Modrinth list or by CurseForge ID, pin them (pinning keeps your manual order of the rest; "Sort by name" is a separate button) and get icons and names from the APIs.
- **Changelog templates.** Placeholders `{version} {mc} {loaders} {date} {project}`, with a separate template per project.
- **Checks before upload.** Warnings for a repeated or lower version number, unknown Minecraft versions, and a file whose loader or Minecraft version differs from the selected ones, plus readable API error messages.
- **Publish history** with search, a failures-only filter, open / copy link for each published version, and CSV export.
- **Auto-update.** An "update available" banner with an Update button (Windows). The download is verified (size, SHA-256 from the release when available), the folder must be writable, and if the new build crashes right after start the previous version is restored.
- **Import and export of projects** without tokens, and a safe config save with a backup copy.
- **Keyboard friendly.** Ctrl/Cmd+Enter publishes (from another section it first opens Publish so you can see the summary), Ctrl/Cmd+1/2/3 switches sections; screen reader labels are provided.

## Install

1. Open [Releases](https://github.com/Waffylka/ModWire/releases/latest).
2. **Windows:** download `modwire.exe` and run it. **macOS:** download `Modwire.dmg` and drag the app into Applications.
3. On first launch open **Settings** and paste your tokens (see below).

After that the app tells you when a new version is available.

### Code signing

The app is **not signed or notarized**:
- Windows shows SmartScreen: click "More info" → "Run anyway".
- macOS shows Gatekeeper: right-click the `.app` → "Open". On macOS 15+ that may not be enough: System Settings → Privacy & Security → "Open Anyway".

## Settings

- **Modrinth:** a Personal access token with the Create versions permission (plus Read user and Read projects to list your projects).
- **CurseForge:** an upload API token (curseforge.com/account/api-tokens).
- **CurseForge Core API key** is optional and only used to fetch a project's name and icon.
- **Where data is stored.** `<config_dir>/mod-publisher/config.json`. **Tokens are stored as plain text** (on Linux/macOS the file is created with 0o600 permissions; on Windows there is no extra encryption and protection comes only from your user profile permissions: the default path is `%APPDATA%\mod-publisher\config.json`, so keep that folder out of cloud sync and do not share it. To remove the tokens press "Clear tokens" or delete the file and `config.backup.json`; revoke any token that may have leaked on the platform's site). Writes are atomic, the previous config is kept as `config.backup.json`, a schema change creates `config.pre-vN.json`, and unknown fields are preserved. "Clear tokens" removes the tokens together with the copies that contain them.
- **History:** `history.jsonl` next to the config (it can be cleared inside the app).
- **Custom data folder.** Set the `MODWIRE_CONFIG_DIR` environment variable to keep the config, history, icons and lock file in another folder (useful for trying the app on empty data without touching your real settings).
- `.jar` and `.mrpack` files are read in memory only (each metadata entry is capped at 1 MB) and nothing is extracted to disk.

## Build from source

```powershell
cargo build --release
cargo test
cargo clippy --all-targets
```

- Windows: a short build path is recommended to avoid LNK1104: `$env:CARGO_TARGET_DIR='C:/mp-target'`.
- macOS: builds on macOS only. GitHub Actions (`.github/workflows/release.yml`) produces the Windows `.exe` and a universal `.dmg` (aarch64 + x86_64).
- Tests cover logic and network scenarios against a local mock; real APIs are never called.
- CI (`.github/workflows/ci.yml`): clippy and tests on Windows and macOS for pushes to `main` and for pull requests.

## Releasing a new version

1. Bump `version` in `Cargo.toml` (for example `0.3.0`).
2. Create a tag with the same version and push it:
   ```powershell
   git tag v0.3.0
   git push origin v0.3.0
   ```
3. `release.yml` builds `modwire.exe` and `Modwire.dmg` and attaches them to the release. If the tag does not match the version in `Cargo.toml`, the build stops.

## Auto-update

The app checks [Releases](https://github.com/Waffylka/ModWire/releases) on startup and on demand (Settings → «Про застосунок» → «Перевірити оновлення»). When a newer version exists, a banner with an Update button appears: the file is downloaded, the app replaces itself and restarts. Automatic replacement works on Windows; on macOS the button opens the release page. Updating is disabled while a publish is running.

Updates are accepted only from `https://github.com/Waffylka/ModWire/releases/download/…`, and the downloaded file is checked for size, SHA-256 and the Windows executable signature. If the release asset carries a SHA-256 digest, the download must match it, otherwise the update is cancelled; the update also needs write access to the folder with `modwire.exe` (it will not work from a protected folder such as Program Files). If the new copy exits with an error within a few seconds of launch, the previous version is put back automatically; the previous file (`modwire.exe.old`) is kept until the new build has run for about 10 seconds (it then writes `update.ok` and the old file is removed). If the new build closes before that, the next launch offers to restore the previous version. There are no code signatures: trust rests on HTTPS and access to this repository. The repository address is the `UPDATE_REPO` constant in `src/logic.rs`.

## Environment variables

- `MODWIRE_CONFIG_DIR` - keep the config, history, icon cache and lock file in another folder instead of `<config_dir>/mod-publisher`.
- `MODWIRE_SHOT_DIR` - developer screenshot mode: saves PNGs of the main screens to this folder. It only starts when `MODWIRE_CONFIG_DIR` is set too, so your real data is never touched.
- `MODWIRE_SHOT_SCALE` - UI scale for screenshot mode (for example `1.0`); `MODWIRE_SHOT_ONLY` limits it to scenes whose name starts with that text.
- `MODWIRE_NO_SPLASH=1` - turn off the animated startup logo (it plays for about 3 seconds; a click or any key skips it). `MODWIRE_SPLASH_AT=<seconds>` freezes it at that moment (used for screenshots).

## Known limitations

- The interface is in Ukrainian only.
- Auto-update replaces the file on Windows only; on macOS the button opens the release page. The build is not code-signed.
- Auto-update verifies SHA-256 only when the release asset reports one.
- A publish interrupted mid-upload may still have reached the site: check the version list before retrying (the app warns about this).
- CurseForge does not accept `.mrpack`; duplicate detection on CurseForge needs the optional Core API key and looks at the latest 500 files.
- Tokens are stored as plain text in the config file.

## Font

Bundled Inter font (SIL Open Font License 1.1, https://github.com/rsms/inter).
