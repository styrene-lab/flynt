# Naming Migration: codex → codyx

## Current State (v0.6.x)

The product was renamed from "Codex" to "Codyx" (Styrene Codex = Codyx) to
avoid collision with OpenAI's `codex` CLI tool. The rename is complete for
all user-facing surfaces but internal infrastructure retains the original
name for backwards compatibility with existing vaults and Apple provisioning.

### What says "Codyx" (user-facing)

| Surface | Value |
|---------|-------|
| GitHub repo | `styrene-lab/codyx` |
| Binary name | `codyx` |
| Window title | Codyx |
| Welcome screen | Codyx |
| Menu bar | Codyx |
| Desktop entry | `codyx.desktop` |
| Homebrew formula | `styrene-lab/tap/codyx` |
| Nix flake pname | `codyx` |
| DMG filename | `Codyx-{VER}-macos.dmg` |
| IPA filename | `Codyx-{VER}.ipa` |
| PKG filename | `Codyx.pkg` |
| Release title | `Codyx {VER}` |
| macOS app bundle | `Codyx.app` |
| iOS display name | Codyx |
| Share extension | "Save to Codyx" |
| Launcher profile path | `~/.config/codyx/` (new installs) |
| Fallback dotfile | `~/.codyx-launcher-profile.json` |
| All docs, README, CHANGELOG | Codyx |
| Archive naming | `codyx-v{VER}-linux-*.tar.gz` |

### What still says "codex" (internal infrastructure)

| Surface | Value | Reason |
|---------|-------|--------|
| Rust crate names | `codex-core`, `codex-app`, etc. | Cargo workspace — rename requires touching every import |
| Vault config dir | `.codex/` | Existing vaults would break |
| Local state dir | `.codex-local/` | Existing state would be orphaned |
| Index database | `codex-index.db` | Would trigger full reindex |
| Bundle ID | `io.styrene.codex` | Apple provisioning profiles are bound to bundle ID |
| App Group | `group.io.styrene.codex` | iOS share extension + AppIntents share data via this group |
| Env vars | `CODEX_VAULT`, `CODEX_LOCAL_STATE`, etc. | Scripts and configs reference these |
| Launcher profile (old) | `~/.config/codex/` | Backwards compat fallback |
| OAuth callback scheme | `codex-oauth` | Registered in Info.plist |
| Keychain service | Referenced via `io.styrene.codex` | Existing tokens stored under this |
| CSS class names | `.codex-shell`, `.codex-body` | Internal, never user-visible |
| Module paths | `codex_core::`, `codex_store::` | Rust internal |

## Migration Plan for 1.0.0

### Phase 1: Crate Rename (breaking, coordinate with release)

Rename all crates in one commit:

```
codex-core    → codyx-core
codex-store   → codyx-store
codex-app     → codyx-app
codex-agent   → codyx-agent
codex-mobile  → codyx-mobile
codex-models  → codyx-models
```

This touches every `use` statement, every `Cargo.toml`, and the workspace
`members` list. Mechanical but high-volume. Use `sed` across the workspace.

The `omegon-extension` dependency references `codex-agent` — update the
Omegon documentation if the extension crate name changes.

### Phase 2: Vault Config Directory

```
.codex/          → .codyx/
.codex-local/    → .codyx-local/
codex-index.db   → codyx-index.db
```

**Migration strategy:**
1. On launch, check for `.codyx/config.toml` first
2. If not found, check for `.codex/config.toml`
3. If found at old path, copy to new path and log a migration notice
4. Support both paths for 2 minor versions, then warn, then drop

### Phase 3: Apple Bundle ID

**Cannot change** without losing:
- TestFlight beta testers (new app, not an update)
- App Store listing (if published)
- Keychain items (stored under old bundle ID)
- App Group shared data (share extension + AppIntents)

**Options:**
- Keep `io.styrene.codex` permanently (Apple doesn't display bundle IDs to users)
- Register `io.styrene.codyx` as a new app and migrate (expensive, loses history)

**Recommendation:** Keep `io.styrene.codex` as the bundle ID. It's invisible
to users. The display name is already "Codyx".

### Phase 4: Environment Variables

```
CODEX_VAULT        → CODYX_VAULT
CODEX_LOCAL_STATE   → CODYX_LOCAL_STATE
CODEX_LAUNCHER_PROFILE → CODYX_LAUNCHER_PROFILE
CODEX_BUILD_HASH    → CODYX_BUILD_HASH
```

**Migration strategy:**
1. Check `CODYX_*` first, fall back to `CODEX_*`
2. Log deprecation warning when old var is used
3. Remove old var support in 2.0.0

### Phase 5: CSS Class Names

```
.codex-shell → .codyx-shell
.codex-body  → .codyx-body
```

Low priority. These are never visible to users or referenced in external
tooling. Rename whenever convenient.

### Phase 6: OAuth Callback Scheme

```
codex-oauth → codyx-oauth
```

Requires updating `Info.plist` on both macOS and iOS. Existing tokens
stored in Keychain under the old scheme key would need migration.

## Sequencing

| Phase | When | Breaking? | Risk |
|-------|------|-----------|------|
| 1. Crate rename | 1.0.0-rc.1 | Yes (internal only) | Low — no external consumers |
| 2. Vault config dir | 1.0.0 | No (fallback) | Low — auto-migration |
| 3. Bundle ID | Never | N/A | Keep as-is |
| 4. Env vars | 1.0.0 | No (fallback) | Low — deprecation warnings |
| 5. CSS classes | 1.0.0 | No | None |
| 6. OAuth scheme | 1.0.0 | Minor | Low — re-auth required |

## Decision Record

- **2026-04-28:** Product renamed from Codex to Codyx to avoid collision
  with OpenAI's `codex` CLI. User-facing surfaces updated immediately.
  Internal infrastructure deferred to 1.0.0 to avoid breaking existing
  vaults and Apple provisioning.
- **Bundle ID stays `io.styrene.codex`:** Apple doesn't expose this to
  users, and changing it loses TestFlight history. Not worth the churn.
