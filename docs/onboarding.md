# Codyx Onboarding, Installation & Migration

Internal reference for tester onboarding. Updated 2026-04-20.

---

## Platforms

| Platform | Artifact | Signing | Distribution |
|----------|----------|---------|--------------|
| macOS | `Codyx-{VER}.dmg` | Developer ID (UZBY9DM42N) | Direct download |
| iOS | `Codyx-{VER}.ipa` | Apple Development (Q4FM48AWU9) | Ad-hoc / TestFlight |

Build both with:
```bash
./scripts/build-release.sh 0.1.0
```

---

## macOS Install Path

1. Download or build `Codyx-{VER}.dmg`
2. Open DMG, drag `Codyx.app` to `/Applications`
3. First launch: Gatekeeper may block — right-click > Open, or `xattr -cr /Applications/Codyx.app`
4. Welcome screen offers 5 vault setup paths (see below)

**Notarization is NOT yet configured.** Testers will need to bypass Gatekeeper on first launch. Notarization requires an app-specific password for `xcrun notarytool submit`.

### Prerequisites
- macOS 13+ (Ventura or later)
- For Git sync: SSH key in `~/.ssh/` (ed25519, rsa, or ecdsa) or Git credential helper configured

---

## iOS Install Path

### Ad-hoc (current)
1. Tester provides their device UDID
2. Add UDID to provisioning profile in Apple Developer portal
3. Build: `./scripts/build-release.sh 0.1.0`
4. Install via Xcode or `xcrun devicectl device install app`
5. Push vault data: `xcrun devicectl device copy to --device <name> --domain-type appDataContainer --domain-identifier io.styrene.codex --source <vault-dir> --destination Documents/Codyx`

### TestFlight (future)
1. Build IPA
2. Upload to App Store Connect via `xcrun altool --upload-app` or Transporter
3. Invite testers via email — no UDID collection needed
4. 90-day expiration per build

### Mobile limitations
- **No onboarding UI** — vault must exist at `Documents/Codyx` before launch
- **Settings are read-only** — all config changes happen on desktop, then sync
- **No git clone on device** — initial vault is pushed via USB or synced after desktop setup
- Requires iOS 17.0+

---

## Vault Setup Paths (Welcome Screen)

### 1. Open existing vault
**Who:** Users migrating from Obsidian or with an existing markdown folder.

1. Click "Open existing vault" > pick folder
2. Codyx creates `.codex/` directory inside the folder
3. SQLite index built from all `.md` files
4. No sync configured — local only until Settings > Sync

**Obsidian compatibility:**
- Wikilinks (`[[note]]`) supported
- TOML frontmatter (`+++...+++`) is Codyx-native; YAML (`---...---`) parsed but not written
- `.obsidian/` directory is ignored
- Obsidian plugins are not supported — Codyx has its own equivalents

### 2. Create local vault
**Who:** New users starting fresh.

1. Click "Create local vault" > pick or create an empty folder
2. Vault initialized with default config and templates
3. No sync — pure local-first

### 3. Clone remote vault
**Who:** Users with an existing Git-backed vault on GitHub/GitLab.

1. Click "Clone remote vault"
2. Enter repository URL (SSH or HTTPS) and branch
3. Codyx clones to `~/Documents/{repo-name}/`
4. Auto-sync configured at 60-second intervals
5. Vault opens with all cloned content indexed

**Auth requirements:**
- SSH: key must be in `~/.ssh/` (id_ed25519, id_rsa, id_ecdsa) or loaded in SSH agent
- HTTPS: Git credential helper must be configured (`git config --global credential.helper osxkeychain`)
- No in-app credential entry — relies on system Git configuration

### 4. Import markdown folder
**Who:** Users who want to bring content into an existing vault.

1. Click "Import" > pick source folder
2. All `.md` files copied into the current vault
3. Wikilinks and structure preserved
4. Non-destructive — source folder unchanged

### 5. Seed demo publication
**Who:** Users exploring the publication feature.

1. Click "Seed demo" > pick folder
2. Scaffolds an Astro site skeleton for publishing from a vault

---

## Git Sync Setup (Post-Install)

For users who chose "Create local vault" or "Open existing vault" and want to add sync later:

1. Initialize git in the vault root: `cd ~/Documents/MyVault && git init && git remote add origin <url>`
2. Open Codyx > Settings > Sync
3. Select "Git", enter remote name (`origin`), branch (`main`), interval (`60`)
4. Save — auto-sync starts immediately

### How sync works
- Background loop every N seconds (configurable, default 60)
- Stages all changes > commits as "Codyx <codex@local>" > pulls (fast-forward or merge) > pushes
- Conflicts detected and reported in the sync status bar
- Exponential backoff on failures (up to 10 minutes)
- Mobile enforces minimum 30-second interval

### Credential flow
Git operations use `git2` with credential callbacks:
1. SSH agent (if running)
2. SSH key files in `~/.ssh/` (ed25519 > rsa > ecdsa)
3. Git credential helper (for HTTPS)

No tokens or passwords are stored by Codyx.

---

## Obsidian Migration Checklist

| Feature | Codyx Status |
|---------|-------------|
| Wikilinks `[[note]]` | Supported |
| Tags `#tag` | Supported (TOML frontmatter) |
| Backlinks | Supported (graph view) |
| Daily notes | Supported (same date format) |
| Templates | Supported (`.codex/templates/`) |
| Canvas/Excalidraw | Excalidraw drawings supported |
| Dataview queries | Codyx query blocks (`TABLE`, `LIST`, `TASK`) |
| Community plugins | Not supported |
| YAML frontmatter | Read but not written (Codyx uses TOML `+++`) |
| Vim mode | Not yet |
| PDF/image embeds | Images embedded; PDF not yet |

### Migration steps
1. Copy or point Codyx at your Obsidian vault folder
2. Codyx indexes everything — `.obsidian/` is ignored
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
2. Push vault to phone via USB (`xcrun devicectl device copy to`)
3. Mobile app reads the vault from `Documents/Codyx`
4. Mobile has read + basic sync; full editing on desktop

Future flow (with StyreneIdentity):
1. Both devices derive the same SSH keys from shared identity
2. Git sync works natively on both platforms
3. No USB push required — clone directly on device

---

## Configuration Files

| File | Location | Purpose |
|------|----------|---------|
| Launcher profile | `~/.local/share/codex/launcher-profile.json` | Last vault, known vaults, wizard state |
| Vault config | `{vault}/.codex/config.toml` | Name, sync, appearance, runtime |
| Omegon profile | `{vault}/.omegon/profile.json` | Model, thinking level, max turns |
| Operator settings | `{vault}/.codex/operator-settings.json` | Persona, skills, Vox, daemon config |
| SQLite index | `~/.local/share/codex/index-{hash}.db` | Document index (auto-rebuilt) |

---

## Known Limitations for Testers

1. **No notarization** — macOS Gatekeeper will warn on first launch
2. **No auto-update** — testers must manually download new builds
3. **No crash reporting** — testers should report issues via Slack/GitHub with console logs
4. **No mobile onboarding** — vault must be pre-configured
5. **SSH keys must be in ssh-agent** — passphrase-protected keys need `ssh-add` first; no in-app passphrase prompt
6. **Single theme** — "alpharius" is the only theme
7. **No Vim mode** — CodeMirror 6 without Vim extension
8. **Commit author is "Codyx <codex@local>"** — not yet linked to user identity (StyreneIdentity planned)
9. **iOS is read-heavy** — editing works but is basic (no CM6 on mobile, plain textarea)
10. **Omegon agent requires separate install** — `omegon` binary must be available at `~/.local/share/omegon/bin/omegon`

---

## Tester Feedback Channels

- GitHub Issues: (repo TBD)
- Console logs: `RUST_LOG=codex_app=debug,codex_store=debug` for verbose output
- Mobile logs: Xcode console when connected via USB

---

## Quick Start for a New Tester (macOS)

```bash
# 1. Install
open Codyx-0.1.0.dmg  # drag to Applications
xattr -cr /Applications/Codyx.app  # bypass Gatekeeper if needed

# 2. SSH key (if not already set up)
ssh-keygen -t ed25519
cat ~/.ssh/id_ed25519.pub  # add to GitHub

# 3. Launch
open /Applications/Codyx.app

# 4. Choose "Clone remote vault"
#    Enter: git@github.com:your-org/your-vault.git
#    Branch: main
#    -> Vault clones, indexes, and opens
```
