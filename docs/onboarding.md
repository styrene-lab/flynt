# Flynt Onboarding, Installation & Migration

Internal reference for tester onboarding. Updated 2026-04-20.

---

## Platforms

| Platform | Artifact | Signing | Distribution |
|----------|----------|---------|--------------|
| macOS | `Flynt-{VER}-macos.dmg`, `Flynt-{VER}-macos.pkg` | Developer ID (UZBY9DM42N) | Direct download |
| iOS | `Flynt-{VER}.ipa` | Apple Development (Q4FM48AWU9) | Ad-hoc / TestFlight |

Build the direct macOS DMG with:
```bash
just release
```

Build the optional direct macOS PKG installer with:
```bash
just release-pkg
```

---

## macOS Install Path

1. Download or build `Flynt-{VER}-macos.dmg` or `Flynt-{VER}-macos.pkg`
2. DMG path: open DMG, drag `Flynt.app` to `/Applications`
3. PKG path: run installer; it installs `Flynt.app` to `/Applications`
4. Welcome screen offers 5 project setup paths (see below)

Direct-download builds are Developer ID signed and notarized when release signing secrets or the local `flynt` notarytool keychain profile are configured.

Local notarization can also use App Store Connect API key env:

```bash
APPLE_API_KEY_P8_B64=... APPLE_API_KEY_ID=... APPLE_API_ISSUER=... just release
```

Direct-download PKG releases require a Developer ID Installer certificate. The
Mac App Store `3rd Party Mac Developer Installer` certificate is not sufficient
for public non-App-Store PKGs.

For the first public beta target, `v0.10.0`, publish both:

- `Flynt-0.10.0-macos.dmg`
- `Flynt-0.10.0-macos.pkg`

### Prerequisites
- macOS 13+ (Ventura or later)
- For Git sync: a GitHub/Codeberg/GitLab personal access token (recommended), or SSH key in `~/.ssh/`

---

## iOS Install Path

### Ad-hoc (current)
1. Tester provides their device UDID
2. Add UDID to provisioning profile in Apple Developer portal
3. Build: `just ios-ipa`
4. Install via Xcode or `xcrun devicectl device install app`
5. Push project data: `xcrun devicectl device copy to --device <name> --domain-type appDataContainer --domain-identifier io.styrene.flynt --source <project-dir> --destination Documents/Flynt`

### TestFlight (future)
1. Build IPA
2. Upload to App Store Connect via `xcrun altool --upload-app` or Transporter
3. Invite testers via email — no UDID collection needed
4. 90-day expiration per build

### Mobile limitations
- **No onboarding UI** — project must exist at `Documents/Flynt` before launch
- **Settings are read-only** — all config changes happen on desktop, then sync
- **No git clone on device** — initial project is pushed via USB or synced after desktop setup
- Requires iOS 17.0+

---

## Project Setup Paths (Welcome Screen)

### 1. Open existing project
**Who:** Users migrating from Obsidian or with an existing markdown folder.

1. Click "Open existing project" > pick folder
2. Flynt creates `.flynt/` directory inside the folder
3. SQLite index built from all `.md` files
4. No sync configured — local only until Settings > Sync

**Obsidian compatibility:**
- Wikilinks (`[[note]]`) supported
- TOML frontmatter (`+++...+++`) is Flynt-native; YAML (`---...---`) parsed but not written
- `.obsidian/` directory is ignored
- Obsidian plugins are not supported — Flynt has its own equivalents

### 2. Create local project
**Who:** New users starting fresh.

1. Click "Create local project" > pick or create an empty folder
2. Project initialized with default config and templates
3. No sync — pure local-first

### 3. Clone remote project
**Who:** Users with an existing Git-backed project on GitHub/GitLab.

1. Click "Clone remote project"
2. Enter repository HTTPS URL and branch
3. For private repos, paste a personal access token (saved automatically for future sync)
4. Flynt clones to `~/Documents/{repo-name}/`
5. Auto-sync configured at 60-second intervals
6. Project opens with all cloned content indexed

**Auth requirements (in priority order):**
- **Personal access token (recommended):** Enter in the clone dialog or save via Settings > Providers. Stored securely in `~/.config/omegon/auth.json` and used automatically for all future operations.
- **Environment variable:** Set `GITHUB_TOKEN`, `CODEBERG_TOKEN`, or `GITLAB_TOKEN`.
- **SSH keys (advanced):** Keys in `~/.ssh/` or loaded in SSH agent work for `git@…` style URLs.
- **System credential helper:** Falls back to `git credential-helper` if nothing else matches.

### 4. Import markdown folder
**Who:** Users who want to bring content into an existing project.

1. Click "Import" > pick source folder
2. All `.md` files copied into the current project
3. Wikilinks and structure preserved
4. Non-destructive — source folder unchanged

### 5. Seed demo publication
**Who:** Users exploring the publication feature.

1. Click "Seed demo" > pick folder
2. Scaffolds an Astro site skeleton for publishing from a project

---

## Git Sync Setup (Post-Install)

For users who chose "Create local project" or "Open existing project" and want to add sync later:

1. Initialize git in the project root: `cd ~/Documents/MyProject && git init && git remote add origin <url>`
2. Open Flynt > Settings > Sync
3. Select "Git", enter remote name (`origin`), branch (`main`), interval (`60`)
4. Save — auto-sync starts immediately

### How sync works
- Background loop every N seconds (configurable, default 60)
- Stages all changes > commits as "Flynt <flynt@local>" > pulls (fast-forward or merge) > pushes
- Conflicts detected and reported in the sync status bar
- Exponential backoff on failures (up to 10 minutes)
- Mobile enforces minimum 30-second interval

### Sync Activity
- Desktop toolbar sync status is clickable for Git projects.
- The Sync Activity panel reports backend, remote, branch, local dirty files, HEAD, and ahead/behind counts when the remote tracking ref is available.
- The panel keeps session-level run state: current phase, last start/finish timestamps, last outcome, and success/failure counts for the running app instance.
- `Sync now` runs a manual auto-commit, pull, and push using the configured Git remote/branch.
- Conflict files from the background sync loop are listed in the panel and can be opened from there when they are indexed markdown documents.
- Non-Git providers do not expose Git diagnostics. iCloud and desktop cloud providers remain filesystem-sync backends from Flynt's point of view.

### Config boundary
- `.flynt/config.toml` is project-synced and contains project sync settings such as Git remote, branch, and auto-commit interval.
- `.flynt-local/` is device-local and gitignored; it contains derived indexes and runtime UI state.
- Operator/theme/provider credentials are device/operator scoped and are not treated as project content.

### Note recovery
- Desktop Notes has a `History` action for the active note.
- `Cmd+P -> Show Note History` opens the same recovery surface.
- History is Git-backed: it lists commits that touched the active note and previews the selected snapshot as a diff against the current note body.
- Recovery is non-destructive. `Restore as copy` writes `Recovered/<note> <commit>.md` and opens that copy in a tab instead of overwriting the active note.
- `Cmd+P -> Create Snapshot` auto-commits, creates a `snapshot-YYYYMMDD-HHMMSS` tag, pushes tags when possible, then opens Note History so the snapshot is part of the same recovery workflow.

### Publication authoring
- The active note Properties panel exposes publication controls for enabled, visibility, slug, and collections.
- Publication edits update only the `[publication]` frontmatter table and preserve unrelated metadata/body content.
- `Export preview` in Properties and `Cmd+P -> Export Publication Preview` run the local static export and show exported/skipped/error counts plus the output path.
- Adapter boundaries are tracked in `design/publication-adapters.md`: local static folder first, GitHub Pages/Astro as delivery layers over the same manifest.

### Bookmarks and saved searches
- Project bookmarks are stored in `.flynt/bookmarks.toml` so they can travel with a synced project.
- `Cmd+P -> Bookmark Current Note` saves the active note.
- `Cmd+P -> Bookmark Current Search` saves the current search query.
- The sidebar Bookmarks section opens note targets directly and restores saved searches by navigating to Search with the saved query populated.
- The bookmark schema already reserves target types for headings, graph filters, canvas files, and drawings; the first UI pass exposes note and search actions.

### Project Lenses
- Project Lenses are Dataview-style saved views over existing indexed project data.
- Lens definitions live in `.flynt/lenses/*.toml` and contain source, filter, column, sort, layout, and limit settings only.
- Lenses do not persist query results, document snapshots, or duplicated metadata.
- `Cmd+P -> Save Search as Lens` writes a search-backed lens definition and opens the Lenses view.
- The first lens UI renders document/task sources as table or list layouts; the full builder remains a later pass.

### Credential flow
Git operations use `git2` with credential callbacks. For **HTTPS URLs** (recommended):
1. Stored personal access token or OAuth token from `~/.config/omegon/auth.json`
2. Environment variable (`GITHUB_TOKEN`, `CODEBERG_TOKEN`, `GITLAB_TOKEN`)
3. System git credential helper

For **SSH URLs** (advanced):
1. SSH agent (if running)
2. SSH key files in `~/.ssh/` (ed25519 > rsa > ecdsa)

Tokens entered during clone are persisted automatically. Tokens can also be managed via Settings > Providers or by asking Omegon (`/login github`).

---

## Obsidian Migration Checklist

| Feature | Flynt Status |
|---------|-------------|
| Wikilinks `[[note]]` | Supported |
| Tags `#tag` | Supported (TOML frontmatter) |
| Backlinks | Supported (graph view) |
| Daily notes | Supported (same date format) |
| Templates | Supported (`.flynt/templates/`) |
| Canvas/Excalidraw | Excalidraw drawings supported |
| Dataview queries | Flynt query blocks (`TABLE`, `LIST`, `TASK`) and Project Lenses |
| Community plugins | Not supported |
| YAML frontmatter | Read but not written (Flynt uses TOML `+++`) |
| Vim mode | Not yet |
| PDF/image embeds | Images embedded; PDF not yet |

### Migration steps
1. Copy or point Flynt at your Obsidian vault folder
2. Flynt indexes everything — `.obsidian/` is ignored
3. Existing YAML frontmatter is read correctly
4. New notes use TOML frontmatter by default
5. Wikilinks work across both formats

---

## Multi-Device Sync

### Desktop <-> Desktop
Git sync handles this automatically. Both machines clone the same repo, auto-sync keeps them current.

### Desktop <-> iOS
Current flow (pre-StyreneIdentity):
1. Set up Git sync on desktop
2. Push project to phone via USB (`xcrun devicectl device copy to`)
3. Mobile app reads the project from `Documents/Flynt`
4. Mobile has read + basic sync; full editing on desktop

Future flow (with StyreneIdentity):
1. Both devices derive the same SSH keys from shared identity
2. Git sync works natively on both platforms
3. No USB push required — clone directly on device

---

## Configuration Files

| File | Location | Purpose |
|------|----------|---------|
| Launcher profile | `~/.local/share/flynt/launcher-profile.json` | Last project, known projects, wizard state |
| Project config | `{project}/.flynt/config.toml` | Name, sync, appearance, runtime |
| Omegon profile | `{project}/.omegon/profile.json` | Model, thinking level, max turns |
| Operator settings | `{project}/.flynt/operator-settings.json` | Persona, skills, Vox, daemon config |
| SQLite index | `~/.local/share/flynt/index-{hash}.db` | Document index (auto-rebuilt) |

---

## Known Limitations for Testers

1. **No notarization** — macOS Gatekeeper will warn on first launch
2. **Manual update install** — Flynt checks GitHub Releases and surfaces a toolbar update badge. Direct-download installs open the newest PKG/DMG when present; Homebrew/Nix/dev installs are routed to the release page. Testers still confirm installation themselves and can skip a noisy version.
3. **No crash reporting** — testers should report issues via Slack/GitHub with console logs
4. **No mobile onboarding** — project must be pre-configured
5. **SSH keys (if used) must be in ssh-agent** — passphrase-protected keys need `ssh-add` first. Using HTTPS with a personal access token avoids this entirely.
6. **Theme import is desktop-only** — Flynt ships Alpharius, Light, and bundled upstream tweak.cn presets. Operators can import tweak.cn JSON themes, public theme URLs, registry slugs, or theme IDs from Settings → Appearance. Mobile still uses its own basic stylesheet.
7. **No Vim mode** — CodeMirror 6 without Vim extension
8. **Commit author is "Flynt <flynt@local>"** — not yet linked to user identity (StyreneIdentity planned)
9. **iOS is read-heavy** — editing works but is basic (no CM6 on mobile, plain textarea)
10. **Omegon agent requires separate install** — Flynt detects `omegon` from an explicit runtime setting, `OMEGON_BIN`, channel installs under `~/.omegon/versions/`, common Homebrew/user-local paths, or `PATH`. If none is found, the agent panel presents an Omegon setup flow with user-local install, binary selection, session retry, Flynt extension setup, provider settings, runtime settings, and recheck actions.

---

## Tester Feedback Channels

- GitHub Issues: (repo TBD)
- Console logs: `RUST_LOG=flynt_app=debug,flynt_store=debug` for verbose output
- Mobile logs: Xcode console when connected via USB

---

## Quick Start for a New Tester (macOS)

```bash
# 1. Install
open Flynt-0.10.0-macos.dmg  # drag to Applications
# Or run Flynt-0.10.0-macos.pkg

# 2. Launch
open /Applications/Flynt.app

# 3. Choose "Clone remote project"
#    Enter: https://github.com/your-org/your-project.git
#    Branch: main
#    Paste a personal access token if the repo is private
#    -> Project clones, indexes, and opens
```
