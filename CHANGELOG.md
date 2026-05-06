# Changelog

## 0.7.0 — 2026-05-06

First release under the new name. **Codyx is now Flynt.** Binaries, bundle IDs, asset paths, and the project site (https://flynt.styrene.io) all migrated. Existing vaults and configuration continue to work unchanged.

### Added
- **Embedded Omegon agent configuration GUI** — configure the agent surface (model, tools, vault scope) directly from the desktop app, no separate config file editing
- **Git login flow** — first-class git authentication for sync, including SSH key selection and credential helper integration
- **Tracing instrumentation** — structured `tracing` spans across vault, sync, and ACP layers for diagnostics

### Changed
- **Rebrand: Codyx → Flynt** across crates (`flynt-models`, `flynt-store`, `flynt-app`, `flynt-agent`, `flynt-mobile`), binaries, desktop entries, and asset paths
- **Bundle ID** remains `io.styrene.flynt` (unchanged from 0.5.0)
- **Homebrew formula** moved to `Flynt` class on `styrene-lab/homebrew-tap`

### Fixed
- **ACP** — corrections to the Agent Control Protocol surface uncovered while wiring the embedded Omegon GUI

## 0.5.0 — 2026-04-23

### Added
- **iOS Share Extension** — save links, text, and images from any iOS app into your Flynt vault via the system Share Sheet
- **Obsidian-style live preview** — CM6 editor now hides markdown syntax (headings, bold, links, tables) and reveals on cursor focus, using the Lezer syntax tree instead of regex
- **Table widget rendering** — markdown tables render as styled HTML tables; click into them to edit raw markdown
- **Frontmatter hiding** — TOML frontmatter is collapsed when cursor is outside it
- **Wikilink click navigation** — Cmd+click on `[[wikilinks]]` navigates to the target note
- **Context menu** — right-click in the editor for formatting options (bold, italic, headings, code blocks, tables, etc.)
- **Rename Save button** — document rename now has an explicit Save button alongside Enter/Cancel
- **TestFlight distribution** — both macOS and iOS builds upload to TestFlight via CI
- **App Store Connect integration** — `just testflight` builds and uploads both platforms

### Changed
- **Bundle ID** — migrated from `com.black-meridian.flynt` to `io.styrene.flynt`
- **CM6 bundle** — rebuilt with live preview extensions (Lezer-based syntax hiding, StateField block decorations)
- **Version** now pulled from `Cargo.toml` automatically in Justfile
- **macOS TestFlight** builds include App Sandbox, JIT, and network entitlements
- **iOS Info.plist** patching uses actool partial plist merge (fixes App Store validation)

### Fixed
- **IndexingConfig** — existing repos opened as vaults no longer get frontmatter injected into source files
- **App Store icon validation** — actool's generated CFBundleIcons plist is now merged properly instead of hand-crafted
- **macOS sandbox crash** — added `network.server` entitlement for Dioxus edit socket
- **Quarantine xattr** — stripped from provisioning profiles before packaging
- **IPA packaging** — uses `xcodebuild -exportArchive` instead of manual zip for correct bundle structure

## 0.4.0 — 2026-04-20

### Added
- Context menus with cut/copy/paste and formatting
- Wikilink rendering and click-to-navigate in CM6 editor
- Graph edge improvements and force layout tuning
- Sticky notes toolbar

## 0.3.0 — 2026-04-18

### Added
- Sticky topbar with compact title
- Excalidraw drawing integration (8MB lazy-loaded bundle)
- Query blocks (TABLE, LIST, TASK inline queries)
- Daily notes with templates

## 0.2.0 — 2026-04-15

### Added
- Kanban boards with task decay model
- Git sync (auto-commit, push, pull)
- Knowledge graph with D3 force layout
- Search with FTS5

## 0.1.0 — 2026-04-10

### Added
- Initial release
- Markdown notes with TOML frontmatter
- Wikilinks and backlinks
- SQLite index with full-text search
- macOS and Linux desktop builds
