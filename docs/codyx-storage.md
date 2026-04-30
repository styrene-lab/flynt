---
id: codyx-storage
title: "Storage Layer"
status: seed
parent: codyx-root
tags: []
open_questions: []
dependencies: []
related: []
---

# Storage Layer

## Overview

SqliteStore + Vault filesystem indexer. SQLite is the index/cache; markdown files on disk are the source of truth. FTS5 full-text search. WAL mode. VaultWatcher via notify/FSEvents for live re-indexing on file changes. VaultStore trait allows future alternative backends.
