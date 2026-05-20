+++
id = "obsidian-parity-validation"
kind = "design_node"
title = "Obsidian parity validation plan"
status = "active"
tags = ["obsidian-parity", "validation", "qa"]

[data]
parent = "obsidian-parity-milestone"
issue_type = "validation"
priority = 1
+++

# Obsidian parity validation plan

This plan validates the PR 5 parity slice covering OP-04 through OP-07:
publication authoring, bookmarks and saved searches, Project Lenses, and page
previews.

Use a throwaway Flynt project with:

- Five normal Markdown notes.
- Several wikilinks between notes.
- One task note.
- One note with a `[publication]` frontmatter table.
- One search query that returns multiple results.
- One `.flynt/lenses/*.toml` file.

## Baseline

1. Launch Flynt against the test project.
2. Run `cargo check -p flynt-app -p flynt-store -p flynt-core`.
3. Confirm the app opens to Notes and existing notes render.
4. Watch logs for panics, repeated reload loops, and watcher storms.

## OP-04 Publication Workflow

1. Open a note.
2. Open the Properties inspector.
3. Toggle publication enabled.
4. Set visibility to `public`, `unlisted`, and `private`.
5. Edit the slug and collections.
6. Confirm only `[publication]` frontmatter changes; the body and unrelated
   frontmatter remain intact.
7. Run `Cmd+P` -> `Export Publication Preview`.
8. Confirm the modal shows exported, skipped, and error counts plus the output
   path.

## OP-05 Bookmarks And Saved Searches

1. Open a note.
2. Run `Cmd+P` -> `Bookmark Current Note`.
3. Confirm `.flynt/bookmarks.toml` is created.
4. Confirm the sidebar Bookmarks section shows the note.
5. Click the bookmark and confirm it opens the note.
6. Search for a term that returns multiple results.
7. Run `Cmd+P` -> `Bookmark Current Search`.
8. Click the saved search bookmark and confirm it opens Search with the query
   populated.
9. Remove a bookmark and confirm the TOML file updates.

## OP-06 Project Lenses

1. Run a search.
2. Run `Cmd+P` -> `Save Search as Lens`.
3. Confirm `.flynt/lenses/search-*.toml` exists and contains only definition
   data, not result rows.
4. Open Lenses from the sidebar or command palette.
5. Confirm the saved lens appears.
6. Confirm table rows match current project data.
7. Click a title row and confirm it opens the matching note.
8. Hand-write a lens with `op = "exists"` and no `value`; confirm it loads.
9. Add a task lens filtering `status = "in_progress"`; confirm matching tasks
   appear.

Known follow-up: malformed lens TOML recovery is tracked in issue #20.

## OP-07 Page Previews

1. In the live editor, hover a `[[wikilink]]`.
2. Confirm the preview appears after a short delay.
3. Move away and confirm it disappears.
4. Press Escape and confirm it dismisses.
5. Hover a rendered wikilink in the preview/source area.
6. Confirm the same preview behavior.
7. Hover sidebar note rows.
8. Hover search results.
9. Confirm previews show title, path, and capped excerpt.
10. Confirm previews do not steal clicks; clicking the previewed link or result
    still navigates correctly.

## Regression Pass

1. Confirm Excalidraw and Canvas wrappers render.
2. Edit a normal note and confirm autosave persists the change.
3. Toggle Live and Source modes; confirm no content loss.
4. Smoke test command palette flows for open note, new note, note history, and
   sync activity.
5. Confirm the sidebar is usable at narrow width.
6. Confirm preview cards do not cause layout shift or block scrolling.

## Automated Commands

```bash
cargo check -p flynt-app -p flynt-store -p flynt-core
cargo test -p flynt-store publication -- --nocapture
cargo test -p flynt-store bookmarks -- --nocapture
cargo test -p flynt-core lens -- --nocapture
cargo test -p flynt-store lenses -- --nocapture
cargo test -p flynt-app note_preview -- --nocapture
git diff --check
```

## Pass Criteria

- No crashes or panics.
- No note body or frontmatter data loss.
- New project files contain definitions only, not cached result rows.
- Navigation still works after hover previews.
- UI remains responsive on a medium-sized project.
