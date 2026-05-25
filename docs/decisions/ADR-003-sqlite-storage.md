# ADR-003: SQLite for all persistent browser storage

## Status

Accepted

## Date

2026-05-20

## Context

Lumen needs persistent storage for: history, bookmarks, read-later notes, cookie jar with TTL, user profiles, knowledge layer FTS index, HTTP resource cache, IndexedDB-equivalent.

The previous plan was to write a custom B-tree KV store. That plan was cancelled: writing and maintaining a custom persistent storage engine with correct ACID semantics, crash recovery, and compaction is months of work that contributes no browser-specific value.

## Decision

Use SQLite via `rusqlite` with feature `bundled` (compiled into the binary, no runtime libsqlite3 dependency) for all disk-persistent storage.

This is **permanent exception #4** in the dependency policy.

SQLite's built-in FTS5 closes the knowledge layer full-text search requirement without a custom inverted index.

**In-memory `InMemoryStorage` is kept** for tests and ephemeral session-scope data. Both implement the same `StorageBackend` trait.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Custom B-tree KV store | Months of work; ACID correctness, crash recovery, compaction — all need to be re-solved. No browser-specific value. |
| `redb` (pure Rust embedded KV) | No FFI, pure Rust — but requires writing our own FTS5 equivalent for the knowledge layer search. Kept as fallback if `rusqlite` FFI becomes problematic. |
| `sled` (LSM-tree) | Same FTS issue as `redb`; less actively maintained. |

## Consequences

- **Positive:** 25 years of SQLite testing (TH3 test suite); FTS5 covers knowledge layer full-text search; standard in major browsers (Firefox places.sqlite, Chromium History, Safari); `bundled` feature compiles SQLite in — no system dependency.
- **Negative:** FFI boundary (unsafe); adds ~1.5 MB to binary size (bundled SQLite); SQLite file format is a long-term commitment.
- **Future:** if `rusqlite` is abandoned or a CVE appears in bundled SQLite — evaluate `redb` + own FTS or a different pure-Rust SQL engine. This is unlikely to be needed.
