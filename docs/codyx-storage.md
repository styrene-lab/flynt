---
id: flynt-storage
title: "Storage Layer"
status: seed
parent: flynt-root
tags: []
open_questions: []
dependencies: []
related: []
---

# Storage Layer

## Overview

SqliteStore + Project filesystem indexer. SQLite is the index/cache; markdown files on disk are the source of truth. FTS5 full-text search. WAL mode. ProjectWatcher via notify/FSEvents for live re-indexing on file changes. ProjectStore trait allows future alternative backends.
