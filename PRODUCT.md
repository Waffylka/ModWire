# Product

<!-- impeccable:product-schema 1 -->

## Platform

desktop (native Windows and macOS)

(Native desktop app built with Rust + egui/eframe. Not web, iOS or Android; the four schema values don't cover it, so this is recorded literally. Visual work must be expressible in egui.)

## Stack

Rust (edition 2024), eframe/egui 0.37 with glow, egui_extras, reqwest (blocking, rustls), rfd, zip, sha2. Source: `src/main.rs`, `src/logic.rs`, `src/i18n.rs`, `src/splash.rs`, `src/ads.rs`, `src/shots.rs`. Ships as `modwire.exe` (Windows) and `Modwire.dmg` (macOS) via GitHub Actions releases.

## Users

Minecraft mod authors, publishing new versions of their own mods. Situation: they just built a release and want it live on Modrinth and CurseForge in one pass, without filling in two web forms.

## Product Purpose

Publish a new version of a mod to Modrinth and CurseForge in one click. The user picks a `.jar` or `.mrpack`; Modwire reads version, name, Minecraft versions and loaders (Fabric, Quilt, Forge, NeoForge) from it, checks them, and uploads to both platforms at once with per-platform progress, cancel and retry. Success: a release goes out correctly on both platforms in under a minute, with no duplicate or half-published version.

## Positioning

One local tool that publishes to both platforms at the same time, reads metadata straight from the mod file, and retries one platform without duplicating the other. Tokens and data stay on the author's machine.

## Operating Context

Used on release day on a Windows or macOS desktop. Tokens (Modrinth personal access token, CurseForge upload token) are pasted into Settings and stored locally in `<config_dir>/mod-publisher/config.json` as plain text. Sections: Publish, Projects, History. Keyboard-driven: Ctrl/Cmd+Enter publishes, Ctrl/Cmd+1/2/3 switches sections. Built-in auto-update (Windows).

## Capabilities and Constraints

- Projects (from Modrinth list or CurseForge ID), pinning, per-project changelog templates with placeholders `{version} {mc} {loaders} {date} {project}`.
- Pre-upload checks: repeated or lower version, unknown Minecraft versions, loader/MC mismatch.
- Publish history with search, failures-only filter, link open/copy, CSV export.
- Project import/export without tokens; atomic config save with backup.
- Screen reader labels exist and must be kept.
- App is unsigned (SmartScreen / Gatekeeper warnings on first run).
- Interface is Ukrainian today; English is to be added so both languages are first-class (`src/i18n.rs` exists). Layouts must hold up for both.
- Undecided: sponsor/ads block (`src/ads.rs`, `Sponsor`) is present in code; whether it must stay was not confirmed.

## Brand Commitments

Name: Modwire. App icon at `assets/icon.ico` / `icon.png`. Bundled fonts in `assets/fonts`. These are binding. Everything else, including the current amber accent in README badges, is not declared binding.

## Evidence on Hand

Working app (about 9k lines of Rust), released builds (`dist/Modwire-0.3.0.exe`), CI and release workflows, bilingual README (English and Ukrainian). No user testimonials, download numbers or reviews are on hand; none should be invented.

## Product Principles

1. Be sure before upload: show version, compatibility and changelog clearly, and warn before anything goes out, never after.
2. Two platforms, one truth: status is shown per platform, a failure on one never hides or duplicates the other.
3. Fast on release day: few steps, keyboard-friendly, no friction between picking the file and publishing.
4. Local and honest about it: tokens and data stay on the machine, and the UI says plainly what is stored where.

## Accessibility & Inclusion

Screen reader labels and full keyboard operation are existing features and must not regress. Text must render Cyrillic and Latin correctly, and strings in both languages must fit their layouts.
