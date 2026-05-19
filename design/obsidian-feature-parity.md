# Obsidian Feature Parity Pass

Status: active backlog
Date: 2026-05-16

Implementation milestone: [Obsidian Parity Foundation](obsidian-parity-milestone.md)

## Purpose

Obsidian is the practical comparison point for Flynt because it sets user
expectations for local-first Markdown knowledge work: notes live as plain files,
links create a navigable knowledge graph, and advanced behavior is layered on
through core plugins, community plugins, themes, sync, and publish.

Flynt should not clone Obsidian wholesale. Flynt's differentiators are project
work, typed tasks, drawings, deterministic agent tooling, Git-backed history,
and Omegon as an integrated operator surface. This pass identifies the parity
gaps that would make an Obsidian user feel friction when moving to Flynt.

Primary upstream references:

- Obsidian overview: https://obsidianmd-obsidian-help.mintlify.app/introduction
- Core plugins list: https://obsidian.md/es/help/plugins
- Canvas: https://obsidianmd-obsidian-help.mintlify.app/plugins/canvas
- Sync: https://obsidian.md/sync
- Sync help index: https://obsidian.md/help/sync
- Publish help index: https://obsidian.md/help/publish
- Developer docs home: https://docs.obsidian.md/Home
- Build a plugin: https://docs.obsidian.md/Plugins/Getting%20started/Build%20a%20plugin
- Plugin/theme manifest: https://docs.obsidian.md/Reference/Manifest
- Vault API guide: https://docs.obsidian.md/Plugins/Vault
- Obsidian Bases plugin view guide: https://docs.obsidian.md/plugins/guides/bases-view
- Plugin load-time guide: https://docs.obsidian.md/plugins/guides/load-time
- Secret storage guide: https://docs.obsidian.md/plugins/guides/secret-storage
- Obsidian community directory: https://community.obsidian.md/

## Current Flynt Coverage

| Obsidian surface | Flynt status | Notes |
| --- | --- | --- |
| Local-first Markdown vault/project | Covered | Project root is canonical; SQLite is a disposable index. |
| File explorer | Covered | Sidebar tree with folders, create, rename, delete. |
| Markdown editor | Covered | CodeMirror live/source modes, GFM tables/tasklists/footnotes, wikilinks, embeds. |
| Wikilinks | Covered | `[[note]]`, aliases, anchors, and local Markdown links are indexed. |
| Backlink graph | Covered | Global graph supports filters, local mode, click-to-open, entity kinds. |
| Search / quick switcher | Covered | Toolbar search and command palette search across commands and note titles. |
| Command palette | Covered | `Cmd+P`; also `Cmd+K` for agent delegation. |
| Daily notes | Covered | `Daily/YYYY-MM-DD.md` with template expansion. |
| Templates | Covered | `.flynt/templates/*.md`, palette creation commands. |
| Tags | Partial | Indexed from frontmatter and visible in search/graph/palette, but no dedicated tag pane. |
| Properties | Partial | Frontmatter and indexed metadata exist; UI is currently task-biased. |
| Project Lenses / Dataview-like views | Partial | Query blocks support `TABLE`, `LIST`, and `TASK`; lenses persist reusable query/display definitions without storing results. |
| Canvas | Partial | JSON Canvas and Excalidraw/Flow exist, but Obsidian-style note/media/web/folder cards need a single operator-facing canvas model. |
| Outgoing links | Partial | Indexed on documents, used by graph, but no active-note pane. |
| Backlinks pane | Partial | Store API exists; no active-note pane with linked and unlinked mentions. |
| Outline | Missing | Headings are parsed for HTML, but no active-note table of contents pane. |
| Bookmarks | Missing | No saved note/search/heading/canvas bookmarks. |
| Workspaces/layouts | Missing in UI | Omegon workspace controls exist separately; Flynt UI does not save/restore operator layouts. |
| Page preview | Missing | No hover preview for wikilinks/search/sidebar links. |
| Note composer | Missing | No merge note / split selection commands. |
| Slash commands | Missing | Agent has slash commands; editor does not expose `/` insert/command flow. |
| Word count | Missing | Not shown for active note or selection. |
| Footnotes view | Missing | Markdown supports footnotes; no navigation pane. |
| Audio recorder | Missing | Not strategically important for current Flynt audience. |
| Slides | Missing | Not strategically important until publication grows. |
| File recovery | Partial | Git snapshots/tags and sync conflict banner exist; no browse/restore UI. |
| Sync | Covered differently | Git/iCloud/local cloud folders instead of Obsidian Sync service. No E2EE sync service by design. |
| Selective sync | Partial | Project scope/indexing config exists; no per-file-type selective sync UI. |
| Publish | Partial | Static preview/export pipeline exists; not Obsidian Publish parity yet. |
| Themes | Covered | tweakcn import and built-ins. CSS snippets are not supported as a formal user surface. |
| Community plugins | Covered differently | Omegon extensions/skills are the extension plane; no Obsidian plugin compatibility. |
| Mobile | Partial | iOS companion and share extension exist; editor is intentionally lighter. |
| Web clipper | Partial | iOS share extension; no browser extension/importer. |
| Importer | Partial | Markdown folder import exists; no broad app-specific importer suite. |
| URI/CLI/headless | Partial | App commands and agent tools exist; user-facing URI/CLI contracts are not documented as compatibility APIs. |

## Sync And Identity Parity

Obsidian Sync is a hosted service. Its parity surface is not "has a sync
button"; it is the combination of private cross-device sync, end-to-end
encryption, version history, selective sync, sync activity, shared vaults,
regional hosting, and headless sync.

Flynt deliberately uses a different foundation:

- Git sync for history, portability, conflict visibility, and audit trail.
- Passive cloud-folder sync for iCloud/Google Drive/Dropbox/OneDrive setups.
- Styrene Identity for local operator identity, SSH/git signing keys, and
  future project sealing.
- Project manifests for multi-project discovery and cloning.

Current Flynt coverage:

| Sync/identity surface | Flynt status | Assessment |
| --- | --- | --- |
| Cross-device sync | Covered differently | Git and local cloud folders work, but setup is more technical than Obsidian Sync. |
| Offline-first | Covered | Project files are local and sync later. |
| Version history | Strong primitive, weak UI | Git has better raw history than snapshots, but Flynt lacks a first-class restore browser. |
| Deleted file recovery | Partial | Git can recover deletes; no user-facing deleted-file recovery UI. |
| Selective sync | Partial | Indexing scopes and cloud folders exist; no per-file-type sync toggles. |
| Settings sync | Partial | `.flynt/config.toml` syncs when project syncs, but device-local settings live elsewhere. |
| Theme/snippet sync | Partial | Theme choice can live in config; imported theme library/runtime state needs a clear sync boundary. |
| Plugin/extension sync | Partial | Omegon extensions are runtime-level, not project-level, unless explicitly managed. |
| End-to-end encrypted sync | Not shipped | Project sealing design exists; not integrated into save/index/sync lifecycle. |
| Shared vault/team collaboration | Non-goal | Flynt stays single-user. Git sharing is possible, but not realtime collaboration. |
| Identity | Differentiator | Styrene Identity and Keychain/Tier B work create a stronger operator identity story than Obsidian's account-centered sync. |

Priority implementation tasks:

1. Build the File Recovery UI over Git history before adding any new sync
   backend. It gives Flynt immediate parity with Obsidian version history and
   recovery while using the existing Git model.
2. Add a Sync Activity view that shows last commit, pull/push result, conflicts,
   remote, branch, and pending local changes. Obsidian users expect this status
   to be inspectable, not just a toolbar glyph.
3. Define synced vs device-local config boundaries:
   - project-synced: project name, publication rules, indexing scopes,
     visualization defaults, selected built-in/custom theme id when portable.
   - device-local: window layout, local paths, tokens, Omegon runtime root,
     installed extension binaries, skipped update version.
4. Turn project sealing from design into a vertical slice:
   - selective sealed note read/write
   - index frontmatter only
   - sync encrypted body as opaque content
   - lock/unlock UI
   - Keychain identity unlock on Apple platforms.
5. Add selective sync policy for generated/heavy content:
   - drawings PNG exports
   - embedded media
   - `.flynt-local`
   - publication output
   - agent artifacts.
6. Document the recommended sync lanes:
   - "simple personal": iCloud/Dropbox/Google Drive folder
   - "auditable/dev": Git
   - "sealed": Git plus selective/full project seal
   - "multi-project": manifest repo.

Strategic conclusion: do not build an Obsidian Sync clone. Flynt's credible
answer is Git-backed history plus Styrene Identity plus optional sealing. The
missing parity is UX around history, activity, selective policy, and identity
onboarding.

## Publication Parity

Obsidian Publish is a hosted product surface: selected notes can be published as
a wiki, knowledge base, documentation site, or digital garden. The official
help surface includes site management, collaboration on publish sites,
customization, custom domains, permalinks, analytics, SEO, and security/privacy.

Flynt's publication model is currently a static export pipeline:

- Publication policy in project config: default visibility plus tag/path rules.
- Per-document publication frontmatter.
- Exported markdown, HTML, micron, manifest, and index artifacts.
- Local preview/export from Settings.
- Bootstrap helper for a GitHub Pages/Astro demo publication target.

Current Flynt coverage:

| Publish surface | Flynt status | Assessment |
| --- | --- | --- |
| Select notes to publish | Partial | `publication.enabled` and rules exist; no authoring affordance in note inspector yet. |
| Public/unlisted/private | Covered | Visibility model exists. |
| Static HTML export | Covered | Local preview emits HTML. |
| Manifest/index | Covered | Export writes `manifest.json` and index. |
| Wikilink rewrite | Covered | Published output rewrites links to published slugs. |
| Custom domain | Not built | Can be handled by target host, but Flynt has no config/UI. |
| Site customization/theme | Partial | Generated output is basic; app themes do not translate into publish themes. |
| Search on published site | Missing | Manifest enables it, but no bundled static search UI. |
| Graph on published site | Missing | Graph data exists in app; not exported as publish artifact. |
| Canvas/drawing publish | Partial | Static sidecars may work, but no explicit publish renderer contract. |
| Analytics/SEO/social previews | Missing | No metadata UI or adapters. |
| Publish collaboration | Non-goal | Could delegate to Git host permissions. |
| One-click hosted publish | Missing | No Flynt hosting service. |

Priority implementation tasks:

1. Add publication controls to the active-note `Properties` inspector:
   enabled, visibility, slug, collections.
2. Promote publication export from Settings-only advanced action to a visible
   `Publish Preview` command with a report view.
3. Define a publish adapter contract:
   - static local folder
   - GitHub Pages
   - Astro adapter
   - future custom adapter.
4. Export a site search index from `manifest.json` plus stripped body text.
5. Export graph payload filtered to published documents.
6. Add SEO fields to frontmatter: description, canonical slug, image, noindex.
7. Add publish theme mapping:
   - inherit app theme tokens as CSS variables
   - allow `publish.css` override
   - keep generated HTML stable and minimal.
8. Decide whether publication output is generated outside the project by
   default. Keeping generated sites out of the synced project avoids pointless
   Git churn unless the operator explicitly wants to commit the site.

Strategic conclusion: Flynt should not need a hosted Publish service to reach
credible parity. A static adapter pipeline with search, graph, theme tokens,
and GitHub Pages/Astro deployment gets most of the practical value while
staying local-first.

## Community And Plugin Posture

Obsidian's ecosystem is the hard moat. The current community directory advertises
thousands of plugins and hundreds of themes, with categories spanning
integrations, files, editing, automation, commands, links, AI, and Markdown.
Official developer docs support TypeScript plugins, themes, CSS variables, and
CSS snippets. Plugins are developed in a vault-local `.obsidian/plugins`
directory, declare a `manifest.json`, and can register commands, views,
Markdown post-processors, editor extensions, saved views, event handlers, and
settings. Obsidian also documents load-time guidance and secret storage.

Flynt's current extension surfaces:

- `flynt-agent` exposes project tools over the Omegon extension protocol and
  can also run as an MCP server.
- Settings has an Omegon extension manager, Armory browser, provider settings,
  skill settings, and schema-driven extension config/secret UI.
- Agent tools can search/read/write documents, manage tasks/boards, inspect
  backlinks, author drawings/flows, and interact with forge integrations.
- App UI is compiled Rust/Dioxus plus vendored JS bundles; there is no stable
  public UI plugin ABI.

This means Flynt already has an automation/integration extension plane, but not
an app-extension ecosystem comparable to Obsidian's.

### Extension API Difficulty

| API layer | Difficulty | Why |
| --- | --- | --- |
| Agent/tool extensions | Low | Already present through Omegon extensions and MCP-style tools. Needs productization, docs, and versioned capability schemas. |
| Project data extensions | Low-medium | Project files are plain Markdown and store APIs are clear. Need stable read/write contracts, transactions, and watcher events. |
| Query/render extensions | Medium | Query blocks and markdown rendering exist. Need sandboxed custom block renderers and deterministic HTML output. |
| Command palette extensions | Medium | Commands can be modeled as data, but need lifecycle, permissions, keybindings, and enable/disable state. |
| Settings-page extensions | Medium | Schema-driven config UI already exists for Omegon; can be reused if extension APIs stay declarative. |
| Editor extensions | High | CodeMirror 6 extensions from third parties inside a Dioxus/wry app imply JS loading, lifecycle isolation, performance budgets, and security review. |
| UI view/pane extensions | High | Arbitrary views require a stable component/plugin host, sandboxing, layout persistence, and crash containment. |
| Theme/snippet ecosystem | Medium | tweakcn tokens make theme import viable. Arbitrary CSS snippets are easy technically but risky for visual breakage and support. |
| Mobile-compatible plugins | High | Native/mobile parity is difficult unless extensions are declarative or tool-only. |
| Marketplace/review/update system | Medium-high | Registry, signing, compatibility, install/update/remove, trust metadata, and permissions are product work more than technical unknowns. |

### Recommended Flynt Extension Strategy

Do not start by offering arbitrary UI plugins. Start with a capability-based
extension API that matches Flynt's architecture and can be safely versioned.

Phase 1: Formalize the existing project/tool extension API.

- Create `docs/extension-api.md`.
- Define `flynt.extension.json` manifest:
  - id, name, version, min_flynt_version
  - capabilities
  - commands
  - tools
  - config schema
  - secrets
  - permissions
  - desktop_only/mobile_safe.
- Version the tool/capability protocol.
- Expose stable capabilities:
  - documents.search/list/read/create/update/move
  - links.backlinks/outgoing
  - tasks.list/get/create/update
  - boards.list/get/create/update
  - graph.read
  - drawings.semantic
  - flow.read/write
  - publication.export
  - sync.status/history.
- Route secrets through the existing provider/secret UI.
- Add compatibility checks and disabled-state explanations.

Phase 2: Add declarative app integrations.

- Command palette commands.
- Settings panels generated from JSON schema.
- Markdown code-block renderers with static HTML output.
- Query providers returning table/list/card data.
- Publication transformers.
- Theme token packs and `publish.css` packages.

Phase 3: Add constrained UI views only after the above is stable.

- Webview-hosted panels with message-passing, not direct Rust/Dioxus component
  injection.
- Explicit permissions and lifecycle hooks.
- Performance budget on load.
- Crash/timeout isolation.
- No direct filesystem access; all project writes go through Flynt APIs.

Phase 4: Consider editor extensions.

- First-party extension points for slash commands, completions, decorations,
  and hover previews.
- Third-party CodeMirror plugins only after a sandbox/loading policy exists.

### Minimum Viable Public API

The first public extension API should be small:

```json
{
  "id": "com.example.flynt-extension",
  "name": "Example Extension",
  "version": "0.1.0",
  "min_flynt_version": "0.11.0",
  "runtime": { "type": "native", "binary": "example-extension" },
  "capabilities": {
    "tools": true,
    "commands": true,
    "settings": true,
    "markdown_renderers": false,
    "ui_views": false
  },
  "permissions": [
    "documents:read",
    "documents:write",
    "tasks:read",
    "tasks:write",
    "network"
  ]
}
```

The key difference from Obsidian: Flynt should make extensions capability-first
and permissioned from day one. Obsidian's ecosystem grew from an Electron/JS app
where plugins can be extremely powerful; Flynt can borrow the ergonomic lessons
without inheriting the same support and security surface immediately.

Strategic conclusion: an Obsidian-scale ecosystem is difficult, but a useful
Flynt-native extension API is very achievable if it starts as declarative
commands/settings/tools/renderers over the existing Omegon extension foundation.
Arbitrary UI/editor plugins should be a later phase, not the starting point.

## Priority Gaps

### P0: Active Note Context Pane

Obsidian users expect the active note to answer these questions without opening
the global graph:

- What links to this note?
- What does this note link out to?
- What headings/sections are in this note?
- What properties/tags does this note carry?

Flynt already has most of the data:

- `ProjectStore::get_backlinks`
- `Document::outgoing_links`
- indexed frontmatter metadata
- rendered Markdown heading information available through parsing

Implementation tasks:

1. Add an active-note inspector panel that can dock on the right side of Notes.
2. Start with tabs: `Links`, `Outline`, `Properties`.
3. `Links` shows backlinks and outgoing links with click-to-open.
4. `Outline` extracts headings from the active note and scrolls or jumps within the editor.
5. `Properties` shows top-level frontmatter plus indexed `[data]` values, read-only first.
6. Add command palette entries: `Toggle Note Inspector`, `Show Backlinks`, `Show Outline`.

This gives the highest parity lift with minimal storage changes.

### P1: Project Lenses Over Existing Query Engine

Project Lenses are Dataview-style saved views over existing project data using
properties, filters, and multiple layouts. Flynt's query blocks cover the
execution concept, but lenses make it discoverable and reusable for operators.
Lens files store definitions only, not results or duplicate metadata.

Implementation tasks:

1. Introduce `.flynt/lenses/*.toml` saved lens definitions.
2. Model fields: name, source, columns, filters, sort, layout.
3. Support layouts initially: table, list, task board link.
4. Render a Lenses view in the app using the existing `ProjectStore` metadata index.
5. Add command palette entries: `Open Lenses`, `Save Search as Lens`.
6. Keep query blocks as the inline Markdown representation; lenses are reusable UI wrappers.

### P1: Bookmarks And Saved Searches

Bookmarks are low-cost and remove a lot of navigation friction.

Implementation tasks:

1. Store bookmarks in `.flynt/bookmarks.toml`.
2. Bookmark targets: note, heading anchor, search query, graph filter, canvas/drawing.
3. Add a sidebar bottom-mode or inspector tab for bookmarks.
4. Add palette commands: `Bookmark Current Note`, `Bookmark Current Search`.

### P1: File Recovery UI

Flynt has a stronger primitive than Obsidian's snapshots because Git already
contains real history. The missing piece is an operator UI.

Implementation tasks:

1. Add a `History` action for the active note.
2. Show recent commits touching that path.
3. Provide read-only diff preview.
4. Provide `Restore as Copy` first; defer destructive restore.
5. Fold existing `Create Snapshot` command into this surface.

### P2: Page Preview

Hover previews make wikilinks usable at Obsidian scale.

Implementation tasks:

1. Add delayed hover preview for rendered wikilinks.
2. Reuse Markdown rendering pipeline but cap content height and strip heavy embeds.
3. Apply to search results and graph node hover after note-link hover is stable.

### P2: Note Composer

Merge/split note workflows are important for long-running research projects.

Implementation tasks:

1. `Split Note at Heading`: create a new note from the current heading section.
2. `Merge Note Into Current`: append another note with provenance separator.
3. Preserve backlinks by inserting a link to the new note at split location.
4. Add tests for frontmatter preservation and link behavior.

### P2: Tag Pane

Tags are currently present but not navigable enough.

Implementation tasks:

1. Add a tag browser from `Project::list_tags`.
2. Click tag opens Search filtered by tag or a saved Base.
3. Show tag counts and nested tag grouping for `a/b/c`.

### P3: Slash Commands, Word Count, Footnotes

These are polish parity items once the note inspector exists.

Implementation tasks:

1. Add `/` command menu inside CodeMirror for headings, callouts, task lists, links, drawings, canvases, templates.
2. Add active note word/character count in the note metadata strip.
3. Add footnotes as another inspector section if the parser can expose them cleanly.

## Canvas Parity Direction

Obsidian Canvas supports infinite space with text, note, media, web page, and
folder cards in `.canvas` JSON. Flynt has multiple visual surfaces:

- Excalidraw for freeform drawing
- JSON Canvas wrappers
- Flow for structured diagrams/workflows
- semantic drawing tools for deterministic agent authoring

The parity target should be a unified operator-facing canvas model, not another
ad hoc drawing path.

Implementation tasks:

1. Define a Flynt canvas capability matrix: text card, note card, media card, web card, folder import, edge, group.
2. Route `.canvas` files through a canvas editor that can round-trip JSON Canvas without loss.
3. Make note/media/web card creation visible in the UI.
4. Index canvas file/note references as graph links where possible.
5. Keep Excalidraw as a drawing embed and Flow as structured workflow, not as replacements for note cards.

## Non-Goals

These are intentionally not parity targets right now:

- Obsidian plugin ABI compatibility.
- Obsidian Sync service clone.
- Realtime collaboration.
- Full mobile desktop parity.
- Audio recorder and slides before the core knowledge navigation gaps are closed.

## Recommended Implementation Order

1. Active Note Context Pane: backlinks, outgoing links, outline, properties.
2. File Recovery UI over Git history.
3. Bookmarks and saved searches.
4. Project Lenses on top of indexed metadata and query blocks.
5. Page preview for wikilinks/search/sidebar.
6. Canvas card parity and reference indexing.
7. Slash commands, word count, footnotes.

The first three items should be treated as the next feature tranche because
they improve daily navigation immediately and reuse storage/index primitives
that already exist.
