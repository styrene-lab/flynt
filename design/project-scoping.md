+++
id = "project-scoping"
kind = "design_node"
title = "Project scoping — opt-in document management for combo directories"
status = "in_progress"
tags = ["indexing", "project", "code-repo", "frontmatter"]

[data]
parent = "flynt-core"
issue_type = "feature"
priority = 1
+++

# Project scoping — opt-in document management for combo directories

## Problem

Flynt indexes every `.md` file under the project root and stamps TOML frontmatter
(UUID, publication config) into each one. When Flynt is opened on a directory
that is also a code repository, this modifies source-controlled files like
`README.md`, `CONTRIBUTING.md`, and crate-level docs — producing hundreds of
unwanted changes in `git status`.

This was observed when Flynt opened the omegon repo: 1,131 `.md` files were
stamped. ~200 `design/` and `docs/` files had their existing YAML frontmatter
replaced with TOML, destroying metadata (titles, status, open questions, tags).

As Flynt is increasingly used alongside code repos (particularly omegon), this
will be a recurring problem.

## Design

### Three-tier file treatment

| Tier | Where | Frontmatter written? | Indexed in SQLite/FTS? |
|------|-------|---------------------|----------------------|
| **Managed** | Inside a scope with `write_frontmatter = true` | Yes — UUID, kind, full entity data | Yes |
| **Discoverable** | Any `.md` outside managed scopes | No — file never touched | Yes — searchable, read-only |
| **Invisible** | Non-`.md`, hidden dirs, `.flynt/` | N/A | No (unchanged from today) |

### Config surface

Extend `IndexingConfig` in `.flynt/config.toml`:

```toml
[indexing]
write_frontmatter = false          # project-wide default (safe for combo dirs)

[[indexing.scopes]]
prefix = "design/"
kind = "design_node"               # auto-assign entity kind to files in this path
write_frontmatter = true           # override: these files are fully managed

[[indexing.scopes]]
prefix = "docs/"
kind = "document"
write_frontmatter = true
```

Fields on each scope:
- `prefix` (required) — path prefix relative to project root, matched against `rel_path.starts_with(prefix)`
- `kind` (optional) — auto-assigned to files without an existing `kind` in their frontmatter
- `write_frontmatter` (optional) — overrides the project-wide default for files under this prefix

When no scopes are configured, behavior is identical to today: the project-wide
`write_frontmatter` flag applies to all files.

### Code repo detection

On first project creation (not on subsequent opens), `Project::open()` checks for
code repo markers at the project root:

**Required**: `.git/` directory exists

**Plus any of**: `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`,
`Makefile`, `CMakeLists.txt`, `pom.xml`, `build.gradle`, `Gemfile`, `mix.exs`,
`flake.nix`, `deno.json`

When detected, the default config is created with `write_frontmatter = false`
instead of `true`. The user can then configure scopes for subdirectories they
want Flynt to manage.

Pure document projects (no `.git` + build manifest) keep the current default of
`write_frontmatter = true`.

### Scope resolution

`IndexingConfig::should_write_frontmatter(rel_path)`:

1. Find the matching scope: longest `prefix` that `rel_path.starts_with(prefix)` matches
2. If a scope matches and has `write_frontmatter` set, use the scope's value
3. Otherwise, fall back to the project-wide `write_frontmatter`

`IndexingConfig::scope_for_path(rel_path)`:

Returns `Option<&IndexScope>` — the longest-prefix-matching scope, or `None`.

`IndexingConfig::file_tier(rel_path)`:

Returns `FileTier::Managed` if `should_write_frontmatter` is true, otherwise `FileTier::Discoverable`.

### Changes to `index_file()`

Current behavior at line 262-268 of project.rs:
```rust
if frontmatter.id.is_none() {
    frontmatter.id = Some(id.0);
    if self.config.indexing.write_frontmatter {
        // write TOML frontmatter back to file
    }
}
```

New behavior:
```rust
if frontmatter.id.is_none() {
    frontmatter.id = Some(id.0);
    if self.config.indexing.should_write_frontmatter(&rel_path) {
        // Auto-assign kind from scope if file has none
        if frontmatter.kind.is_none() {
            if let Some(scope) = self.config.indexing.scope_for_path(&rel_path) {
                if let Some(ref k) = scope.kind {
                    frontmatter.kind = Some(k.clone());
                }
            }
        }
        // write TOML frontmatter back to file
    }
}
```

All files — managed or discoverable — are still parsed and saved to SQLite.
The only difference is whether the source file is modified.

### Backwards compatibility

- `scopes` uses `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
- Existing configs with no `[[indexing.scopes]]` deserialize with `scopes: vec![]`
- Empty scopes = project-wide `write_frontmatter` applies to all files (today's behavior)
- Code repo detection only fires on first project creation, not on existing projects

### What does NOT change

- `walk_markdown()` — still walks all `.md` files recursively. The tier distinction
  happens in `index_file()`, not during discovery.
- File watcher — still watches all `.md` changes. Discoverable files still trigger
  re-indexing into SQLite when modified externally.
- `save_document_content()` — users can edit any file in the UI. The scope system
  only controls whether `index_file()` writes frontmatter back.
- `set_document_kind()` — should refuse to modify Discoverable files (they're
  not owned by Flynt).

## Implementation

### Phase 1: Model types
- Add `IndexScope`, `FileTier` to `flynt-core/src/models.rs`
- Expand `IndexingConfig` with `scopes` field and helper methods

### Phase 2: Code repo detection
- In `Project::open()` default-config branch, detect `.git` + build manifest
- Set `write_frontmatter = false` for new projects in code repos

### Phase 3: Per-file frontmatter decision
- Change `index_file()` to use `should_write_frontmatter(&rel_path)`
- Add scope-based kind auto-assignment

### Phase 4: Guard mutations
- `set_document_kind()` returns error for Discoverable files

### Phase 5: Settings UI (deferred)
- Scope list editor in settings panel
- Visual indicator for code-repo projects
