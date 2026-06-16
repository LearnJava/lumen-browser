# ADR-012: Storage partitioning — multiple SQLite DBs by lifecycle, KV only for measured blob caches

## Status

Accepted (extends [ADR-003](ADR-003-sqlite-storage.md))

## Date

2026-06-16

## Context

[ADR-003](ADR-003-sqlite-storage.md) settled SQLite as the storage engine for all
persistent browser state. As the number of stores grows (~40 in `lumen-storage`) two
recurring questions came up:

1. Should data be split across **multiple database files** instead of one big DB — for
   size/speed?
2. Should we add a **super-fast embedded key/value engine** (sled / redb / lmdb / rocksdb)
   for some workloads?

Constraints: Lumen is privacy-focused, lightweight, prefers pure-Rust deps (MSVC on
Windows), and follows the two-tier dependency policy ([ADR-002](ADR-002-dependency-policy.md)).
Data volumes today are tiny (bookmarks, history, settings, ad-block subscriptions). The
one genuinely hot path — ad-block `should_block(url)` on every request — already runs
against an in-memory index, not the disk.

A common misconception to dispel: **"smaller DB → faster query" is false for SQLite.**
Query speed is governed by indexes (B-tree, O(log n)), not file size. SQLite is equally
fast on 1k and 10M indexed rows. So splitting *for size* buys nothing.

## Decision

**1. Default stays SQLite (bundled rusqlite), one store per logical concern.**

**2. Partition into multiple DB *files* by access pattern, NOT by table count or size.**
Group tables into a DB file by:
- **Durability / lifecycle:** durable user data (bookmarks, history, settings, ad-block
  subscriptions) vs disposable cache (DNS cache, HTTP-cache metadata, favicons) — separate
  files so cache can be cleared/vacuumed/corrupted independently of user data.
- **Write frequency:** SQLite holds a single writer lock *per database file*. Keep
  high-frequency writers (history, site-engagement) out of the same file as cold data
  (settings) to avoid mutual write-lock contention. This is the main real reason to split.

Anti-pattern to avoid: one-DB-per-table. JOINs and atomic transactions only span a single
file; hundreds of tiny DBs waste handles/connections and lose cross-table atomicity.

Already in practice: IndexedDB is sharded per origin (`{sha256(eTLD+1)[:16]}.db`).

**3. No second storage engine without a measured bottleneck.** A key/value engine
(redb preferred — pure-Rust, ACID, mmap, single file; sled rejected — beta on-disk format;
lmdb/rocksdb rejected — C/C++ deps) is introduced **only** for a specific pure `key → blob`,
high-throughput cache **after profiling shows SQLite is the bottleneck there**, and **only
via its own ADR**. Candidate future workloads: HTTP-cache bodies, favicon cache, persisted
glyph atlas — all pure blob lookups. Not adopted now.

**4. Never put a per-request hot path on disk.** Matching/lookup that runs on every request
(e.g. ad-block `should_block`) stays in RAM (`EasyListFilter` HashMap). Neither SQL nor KV
belongs there.

**Worked example — ad-block (`docs/tasks/p2-adblock-filter-lists.md`):** a small dedicated
`data/adblock/adblock.db` holds `subscriptions` + `list_meta` (structured, queryable, atomic);
large list bodies live as `lists/*.txt` files (read once, parsed to RAM); the matcher is in
RAM. The dedicated DB is for tidiness and folder structure, not performance.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| One monolithic SQLite DB for everything | Couples write locks of hot and cold data; one corruption/vacuum affects all; can't clear cache independently |
| One DB per table | Loses cross-table JOIN/transaction atomicity; handle/connection overhead; no real speed gain |
| Split DBs "for size/speed" | Misconception — indexed SQLite is size-insensitive; splitting must be justified by lifecycle/write-contention, not size |
| Adopt sled/redb broadly as KV | Premature without a measured bottleneck; second format to back up/migrate/corrupt; fragments the unified SQLite layer; sled on-disk format still beta |
| rocksdb / lmdb | C/C++ build deps; heavy; against pure-Rust + lightweight goals |

## Consequences

- **Positive:** clear rule for where new data lives (SQLite, grouped by lifecycle/write-frequency); avoids both the monolith and the one-DB-per-table sprawl; keeps the storage layer unified and easy to back up; no speculative engines.
- **Negative / trade-offs:** cross-file data needs `ATTACH` or app-level coordination (no cross-file transactions); developers must classify new data by lifecycle/write-frequency rather than defaulting to "a new DB per feature."
- **Future:** introducing a KV engine (redb) for a specific blob cache is a separate ADR, gated by a benchmark showing SQLite as that cache's bottleneck. Until then, SQLite only.
