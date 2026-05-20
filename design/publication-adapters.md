+++
id = "publication-adapters"
kind = "design_node"
title = "Publication adapter contract"
status = "active"
tags = ["publication", "adapters", "obsidian-parity"]

[data]
parent = "obsidian-parity-milestone"
issue_type = "design"
priority = 2
+++

# Publication adapter contract

Flynt publication starts as a local, deterministic static export. Adapters are
thin delivery layers over that export, not alternate renderers.

## Core contract

Every adapter receives:

- `output_root`: the already-rendered local preview directory.
- `manifest.json`: exported documents with title, slug, source path, output
  path, tags, visibility, and generation timestamp.
- generated assets: Markdown, HTML, Micron, index files, and future search/graph
  artifacts.

Adapters must not mutate source notes. They can copy, transform file paths, add
host-specific metadata, or publish the rendered tree.

## Static Folder

The static folder adapter is the baseline implementation.

- Input: local `site_dir` from publication target or `site/`.
- Behavior: export into that folder.
- Success criteria: report exported/skipped/error counts and output path.
- Failure modes: duplicate slugs, renderer errors, filesystem write failures.

## GitHub Pages

GitHub Pages is a delivery adapter over the static folder output.

- Input: repo, branch, optional site directory.
- Behavior: export locally, commit generated output, push to target branch.
- Safety: refuse dirty generated output unless the adapter owns the target
  directory; never push note source unless it is already the configured sync
  remote.
- Future: support `gh-pages` branch and `/docs` folder modes.

## Astro

Astro should consume Flynt's publication manifest rather than re-parsing notes.

- Input: exported manifest and document artifacts.
- Behavior: copy artifacts into an Astro project or generate a content
  collection from the manifest.
- Boundary: Astro owns layout/components; Flynt owns note selection, wikilink
  rewriting, visibility, graph/search payloads, and deterministic exported
  content.

## Compatibility

Adapters are versioned against the manifest schema. A future adapter manifest
should declare:

- supported `publication_manifest` version.
- required generated artifacts.
- write destinations.
- network permissions.
- whether it can run in preview-only mode.
