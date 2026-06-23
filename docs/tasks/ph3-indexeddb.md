# Ph3 — IndexedDB

**Developer:** P1
**Branch:** `p1-ph3-indexeddb`
**Size:** XL (new native Rust-SQLite storage layer, new `lumen-js` primitives, shell wiring)
**Crates:** `lumen-storage`, `lumen-js`, `lumen-shell`

---

## Status

**Phase 3 future (v1.0).** Greenfield task — not started. Listed here as a design reference
so the architecture is settled before implementation begins.

---

## Goal

Implement IndexedDB API (W3C Indexed Database API 3.0) with durable SQLite-backed persistence
per origin. The current JS shim (`crates/js/src/dom.rs:9452`) already implements the full
in-memory API surface (IDBFactory, IDBDatabase, IDBObjectStore, IDBTransaction, IDBRequest,
IDBIndex, IDBCursor, IDBKeyRange, versioned upgrades, cursor iteration). What is missing is:

1. **Native Rust-SQLite storage layer** — replace the opaque-JSON-snapshot approach in
   `crates/storage/src/indexed_db.rs` with a structured schema: one SQLite file per origin,
   one table per IDB object store, a separate index tracking table, and proper schema
   migrations via `versionchange` transactions.
2. **Richer `IdbBackend` trait** — the current `IdbBackend` trait
   (`crates/core/src/ext.rs:1820`) exposes only `load() -> Option<String>` / `save(&str)`
   (opaque JSON blob round-trip). Replace it with a structured API that matches the actual
   IDB request model so the JS shim can call Rust directly instead of serialising the whole
   in-heap database on every flush.
3. **Shell per-origin routing** — wire the new backend into the tab lifecycle so each
   navigation gets the correct per-origin SQLite file.

---

## Current state

### Storage primitives (ground truth in real code)

| Component | File:line | Description |
|---|---|---|
| `IdbStore` (existing) | `crates/storage/src/indexed_db.rs:61` | Opaque-JSON backend: stores entire IDB heap as one blob per origin key |
| `IdbBackend` trait | `crates/core/src/ext.rs:1820` | `load() -> Option<String>` + `save(&str)` — blob round-trip only |
| `origin_key()` | `crates/storage/src/indexed_db.rs:38` | `sha256(eTLD+1)[:16]` → safe filename |
| `IdbStore::for_origin()` | `crates/storage/src/indexed_db.rs:97` | Per-origin SQLite file at `idb_dir/{key}.db` |
| `SqliteStorage` (pattern) | `crates/storage/src/sqlite_store.rs:29` | `Mutex<Connection>`, WAL+NORMAL, `IF NOT EXISTS` init batch |
| `Bookmarks` (pattern) | `crates/storage/src/bookmarks.rs:46` | Multi-table SQLite with FK + CASCADE, `execute_batch` schema init |
| `PrintPrefs` (pattern) | `crates/storage/src/print_prefs.rs:87` | Flat KV table pattern using `INSERT OR REPLACE` |
| `lumen_idb_dir()` | `crates/shell/src/main.rs:4304` | **Currently uses APPDATA/HOME paths, not portable `<exe>/data/` pattern** |

### JS shim — already implemented (do not rewrite)

The JS layer at `crates/js/src/dom.rs:9452–10393` is fully spec-compliant. Key components:

| Symbol | Location | Notes |
|---|---|---|
| `_idb_databases` (JS var) | `dom.rs:9460` | In-heap store; serialised to JSON by `_idb_serialize()` |
| `_idb_serialize()` / `_idb_deserialize()` | `dom.rs:9472–9486` | Date-tagged JSON; used by the blob persistence path |
| `_idb_persist_if_dirty()` | `dom.rs:9490` | Calls `_lumen_idb_persist(snapshot)` Rust primitive when dirty |
| `_lumen_idb_flush()` | `dom.rs:10317` | Drains pending opens + active transactions, then persists |
| `indexedDB.open()` | `dom.rs:10328` | Full versioned open including `upgradeneeded` |
| `IDBObjectStore.prototype._write` | `dom.rs:9910` | Add/put with keyPath, autoIncrement, constraint check |
| Cursor iteration | `dom.rs:10149–10271` | openCursor / openKeyCursor / advance / continue / update / delete |
| IDBIndex read/cursor methods | `dom.rs:10049–10147` | Full index materialisation at query time |

### Async delivery model (existing, reuse)

IDB requests fire asynchronously via `queueMicrotask(_lumen_idb_flush)` (JS side, `dom.rs:9776`).
The shell calls `tick_timers()` each `about_to_wait` (`shell/src/main.rs:2097`), which calls
`eval_js("_lumen_tick_timers()")`. Microtasks run as part of QuickJS evaluation. No separate
Rust async infrastructure is needed — the existing microtask pump is sufficient.

### Rust primitives bound into JS (existing)

```
_lumen_idb_load  → IdbBackend::load()   (dom.rs:1666)   registered when idb_backend is Some
_lumen_idb_persist → IdbBackend::save() (dom.rs:1668)   registered when idb_backend is Some
```

If `idb_backend` is `None` the shim operates in ephemeral in-heap-only mode (unit tests).
The proposed structured backend is a new implementation of the same `IdbBackend` trait.

### Storage routing in the shell

```
lumen_idb_dir()           shell/src/main.rs:4304  — resolves IDB directory path
TabState.idb_dir          shell/src/main.rs:3648  — per-tab IDB directory
IdbStore::for_origin()    storage/src/indexed_db.rs:97  — creates/opens per-origin .db file
install_dom_api(…, idb_backend, …)  dom.rs:250  — passes backend into JS context
```

---

## Architecture

### Option A: Keep opaque-JSON, improve durability (simpler, lower priority)

Retain `_idb_serialize` / `_idb_deserialize` but fix two known weaknesses:
1. The entire IDB state is one BLOB in the `kv` table — competes with all other origin data.
2. `lumen_idb_dir()` currently uses OS dirs (`%APPDATA%`, `$HOME/.config`) instead of the
   portable `<exe>/data/` pattern mandated by ADR-012 / the adblock precedent.

Steps: (a) redirect `lumen_idb_dir()` to `browser_data_dir().join("idb")` using the pattern
from `p2-adblock-filter-lists.md`; (b) keep `IdbStore` as-is; (c) add a quota guard on
snapshot size (warn at >5 MB per origin).

This is a viable Phase 2.5 interim fix for production correctness, not the full Phase 3 goal.

### Option B: Structured SQLite backend (Phase 3 goal)

Replace the JSON-blob approach with a first-class relational schema inside each per-origin
SQLite file. The JS shim and `IdbBackend` trait are extended so that mutating operations
go directly to SQLite row-by-row, not through a full-heap serialise/deserialise cycle.

#### Schema per origin file (`{sha256(eTLD+1)[:16]}.db`)

```sql
-- Database registry (versioned IDB databases within this origin)
CREATE TABLE idb_meta (
    db_name    TEXT PRIMARY KEY,
    version    INTEGER NOT NULL DEFAULT 1
);

-- Object stores (created/deleted during upgradeneeded)
CREATE TABLE idb_stores (
    db_name       TEXT NOT NULL,
    store_name    TEXT NOT NULL,
    key_path      TEXT,              -- NULL = out-of-line; '' = value itself; dotted string
    auto_inc      INTEGER NOT NULL DEFAULT 0,
    key_gen       INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (db_name, store_name)
);

-- Indexes on object stores
CREATE TABLE idb_indexes (
    db_name       TEXT NOT NULL,
    store_name    TEXT NOT NULL,
    index_name    TEXT NOT NULL,
    key_path      TEXT NOT NULL,
    is_unique     INTEGER NOT NULL DEFAULT 0,
    multi_entry   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (db_name, store_name, index_name)
);

-- Records (one row per stored value; key and value are serialised as JSON)
-- Naming convention: one table per (db_name, store_name) pair would be ideal
-- for query performance but complicates dynamic schema management. Using a
-- single partitioned table is simpler to implement and consistent with the
-- existing lumen-storage KV schema (`sqlite_store.rs`).
CREATE TABLE idb_records (
    db_name     TEXT NOT NULL,
    store_name  TEXT NOT NULL,
    key_json    TEXT NOT NULL,   -- IDB key serialised per _idb_serialize key encoding
    value_json  TEXT NOT NULL,   -- structured clone serialised to JSON (same encoding as _idb_serialize)
    PRIMARY KEY (db_name, store_name, key_json)
) WITHOUT ROWID;

CREATE INDEX idb_records_store ON idb_records (db_name, store_name, key_json);
```

Key design choices:
- **One SQLite file per origin.** Matches the existing `IdbStore::for_origin()` pattern
  (`indexed_db.rs:97`) and WAL parallel-reader semantics. Multiple tabs on the same origin
  share the file safely (SQLite WAL allows one writer + N readers concurrently).
- **`WITHOUT ROWID` on `idb_records`** — the primary key is (db, store, key) which is the
  natural lookup key; avoids a separate rowid B-tree.
- **JSON-serialised keys and values** — reuses the existing `_idb_serialize` Date-tagging
  convention from the JS shim, keeping the Rust side free of JS value type knowledge.
  Binary data (`ArrayBuffer`, `Blob`) requires a future extension (base64 or a side-channel
  blob store); out of scope for Phase 3.
- **`idb_records` as a single partitioned table** — consistent with the existing `kv` table
  in `sqlite_store.rs:59` which uses `(origin, top_level_site, key)` as composite PK. The
  query footprint for IDB is narrow (always keyed by db+store+key range), so a single table
  with a compound index is correct here per ADR-012 §anti-pattern.

#### New `IdbBackend` trait methods (proposed)

```rust
// crates/core/src/ext.rs — proposed additions to IdbBackend (PROPOSED)
pub trait IdbBackend: Send + Sync {
    // --- existing blob path (kept for backward compat + unit tests) ---
    fn load(&self) -> Option<String>;
    fn save(&self, snapshot: &str);

    // --- structured path (Phase 3) ---

    /// Return the stored version of `db_name` for this origin, or 0 if unknown.
    fn db_version(&self, db_name: &str) -> u32 { let _ = db_name; 0 }

    /// List all (db_name, version) pairs for this origin.
    fn list_databases(&self) -> Vec<(String, u32)> { vec![] }

    /// Persist schema changes from a versionchange transaction:
    /// bump version, upsert store/index definitions, drop deleted ones.
    fn apply_schema(&self, op: &IdbSchemaOp) -> lumen_core::Result<()> {
        let _ = op; Ok(())
    }

    /// Execute one read/write operation inside a transaction.
    fn exec_op(&self, op: &IdbRecordOp) -> lumen_core::Result<IdbOpResult> {
        let _ = op; Ok(IdbOpResult::None)
    }

    /// Atomically commit a batch of write operations (one IDBTransaction).
    fn commit_txn(&self, ops: &[IdbRecordOp]) -> lumen_core::Result<()> {
        let _ = ops; Ok(())
    }
}
```

The default implementations (no-ops returning empty/`Ok`) mean existing `IdbStore`
continues to compile unchanged; only `NativeIdbStore` (new) overrides the structured methods.

#### Key ranges → SQL

| IDB operation | SQL translation |
|---|---|
| `get(key)` | `WHERE db_name=? AND store_name=? AND key_json=?` |
| `getAll(range)` | `WHERE … AND key_json BETWEEN ? AND ?` (approximate; exact boundary via app filter) |
| `count(range)` | `SELECT COUNT(*)` with same WHERE |
| `delete(range)` | `DELETE WHERE …` |
| `openCursor(range, direction)` | `SELECT … ORDER BY key_json ASC/DESC` — cursor materialised at open time |
| `index.get(range)` | `SELECT … WHERE value_json LIKE …` (JSON-extract via SQLite `json_extract`) |

> Note on index queries: SQLite `json_extract(value_json, '$.field')` enables indexed
> lookups on IDB index key paths without storing a separate index table, at the cost of a
> full store scan for non-indexed paths. For Phase 3 a full-scan fallback is acceptable;
> a SQLite generated-column index (`CREATE INDEX ON idb_records (json_extract(value_json, '$.id'))`)
> can be added per-index once the schema layer is in place.

#### Transaction model

IDB transactions map directly to SQLite transactions:
- `readonly` → `BEGIN DEFERRED` (allows parallel readers via WAL)
- `readwrite` → `BEGIN IMMEDIATE` (blocks other writers, allows readers)
- `versionchange` → `BEGIN EXCLUSIVE` (schema migrations)

The JS shim collects all requests in `txn._queue` and delivers them at flush time
(`_lumen_idb_flush`, `dom.rs:10317`). The Rust backend receives them as a batch via
`commit_txn(&[IdbRecordOp])`, wrapping them in a single SQLite transaction for atomicity
and performance.

#### Async IDBRequest ↔ JS job queue

No changes needed. The existing delivery model works:

```
JS: indexedDB.open() / store.put()
  → _idb_schedule_flush() → queueMicrotask(_lumen_idb_flush)
Shell: about_to_wait → tick_timers() → eval_js("_lumen_tick_timers()")
  → QuickJS runs microtask queue → _lumen_idb_flush() fires
  → calls _lumen_idb_persist(snapshot) Rust primitive
    (or, Phase 3: calls _lumen_idb_exec_op / _lumen_idb_commit_txn per request)
```

The only change in Phase 3 is that `_lumen_idb_persist` is supplemented (or replaced) by
finer-grained Rust primitives that write individual records rather than a full snapshot.

#### Per-origin routing

```
Navigation → extract eTLD+1 from page URL
  → NativeIdbStore::for_origin(etld_plus_one, idb_dir)
    → origin_key(etld_plus_one)   (indexed_db.rs:38 — reuse as-is)
    → path = idb_dir / "{key}.db"
    → NativeIdbStore::open_or_create(path)
  → install_dom_api(…, Some(Arc::new(native_store)), …)
```

The `idb_dir` value must be migrated from `%APPDATA%/lumen/idb/` to
`<exe>/data/idb/` using `browser_data_dir()` (see `p2-adblock-filter-lists.md:47`
for the helper pattern). This is a prerequisite change in `shell/src/main.rs:4304`.

---

## Entry points

Real entry points (existing, confirmed by grep):

| Symbol | File:line | Role |
|---|---|---|
| `IdbBackend` trait | `crates/core/src/ext.rs:1820` | Extend with structured methods |
| `IdbStore` struct | `crates/storage/src/indexed_db.rs:61` | Keep for ephemeral/test mode; new `NativeIdbStore` for production |
| `origin_key()` | `crates/storage/src/indexed_db.rs:38` | Reuse unchanged |
| `IdbStore::for_origin()` | `crates/storage/src/indexed_db.rs:97` | Mirror for `NativeIdbStore::for_origin()` |
| `_lumen_idb_load` primitive | `crates/js/src/dom.rs:1666` | Keep + add structured primitives alongside |
| `_lumen_idb_persist` primitive | `crates/js/src/dom.rs:1668` | Keep for blob fallback + supplement with structured calls |
| `_lumen_idb_flush()` JS function | `crates/js/src/dom.rs:10317` | Extend to call structured Rust primitives per-request |
| `lumen_idb_dir()` | `crates/shell/src/main.rs:4304` | Fix to use `<exe>/data/idb/` |
| `TabState.idb_dir` | `crates/shell/src/main.rs:3648` | Already per-tab; no structural change needed |
| `install_dom_api` call site | `crates/shell/src/main.rs:2532` | Pass `NativeIdbStore` instead of `IdbStore` |
| `_idb_serialize()` / `_idb_deserialize()` | `crates/js/src/dom.rs:9472–9486` | Reuse for value serialisation in native ops |
| `_idb_databases` JS var | `crates/js/src/dom.rs:9460` | In Phase 3 still used as read cache; writes go to Rust immediately |

Proposed new symbols (PROPOSED — do not exist yet):

| Symbol | File | Role |
|---|---|---|
| `NativeIdbStore` | `crates/storage/src/indexed_db.rs` | Structured SQLite backend |
| `IdbSchemaOp` | `crates/core/src/ext.rs` | Schema operation: create/delete store/index |
| `IdbRecordOp` | `crates/core/src/ext.rs` | Record operation: put/add/delete/clear |
| `IdbOpResult` | `crates/core/src/ext.rs` | Result of a single IDB operation |
| `_lumen_idb_exec_op` | `crates/js/src/dom.rs` | New Rust primitive: single record op |
| `_lumen_idb_commit_txn` | `crates/js/src/dom.rs` | New Rust primitive: commit batch |
| `_lumen_idb_schema_op` | `crates/js/src/dom.rs` | New Rust primitive: schema change |

---

## Steps

### Step 0 — Fix portable data directory (prerequisite)

- In `crates/shell/src/main.rs:4304` change `lumen_idb_dir()` to use `browser_data_dir()`
  (the helper from `p2-adblock-filter-lists.md` / `adblock.rs` pattern): path becomes
  `<current_exe parent>/data/idb/` instead of OS dirs.
- Existing `IdbStore::for_origin()` and all per-origin file paths are unaffected (they
  receive the directory as argument); only `lumen_idb_dir()` changes.
- Tests: verify `lumen_idb_dir()` returns a subpath of `current_exe` parent dir.

### Step 1 — Extend `IdbBackend` with structured methods

- In `crates/core/src/ext.rs` add `IdbSchemaOp`, `IdbRecordOp`, `IdbOpResult` types and
  the four new methods to `IdbBackend` with default no-op implementations.
- Existing `IdbStore` compiles unchanged (defaults cover it).
- `clippy -p lumen-core` must pass.

### Step 2 — Implement `NativeIdbStore`

New struct in `crates/storage/src/indexed_db.rs`:

- `NativeIdbStore::open_or_create(path: &Path) -> Result<Self>`
  - `execute_batch` to create `idb_meta`, `idb_stores`, `idb_indexes`, `idb_records` tables
    (WAL + NORMAL, `IF NOT EXISTS`). Pattern mirrors `bookmarks.rs:69`.
- `NativeIdbStore::for_origin(etld_plus_one: &str, idb_dir: &Path) -> Result<Arc<dyn IdbBackend>>`
  - Same as `IdbStore::for_origin` but returns a `NativeIdbStore`.
- Implement `IdbBackend`:
  - `load()` / `save()`: for backward compat, read/write the snapshot as a BLOB in
    `idb_records` under a synthetic `__snapshot__` key. Allows the JS shim's blob path
    to work unchanged while the structured path is developed.
  - `db_version()`: `SELECT version FROM idb_meta WHERE db_name=?`.
  - `list_databases()`: `SELECT db_name, version FROM idb_meta`.
  - `apply_schema()`: execute DDL for store/index creation/deletion inside a single
    `BEGIN EXCLUSIVE` transaction.
  - `exec_op()`: execute a single `IdbRecordOp` (used for read-only ops in immediate mode).
  - `commit_txn()`: wrap a batch of `IdbRecordOp` in a single `BEGIN IMMEDIATE` transaction.
- Unit tests covering: open+roundtrip, version bump, store CRUD, key range query, index
  insertion and retrieval, multi-origin isolation (two `NativeIdbStore` instances on
  different paths).

### Step 3 — Add Rust primitives to the JS/Rust bridge

In `crates/js/src/dom.rs` in the `install_dom_api` function (around line 1658):

- Register `_lumen_idb_schema_op(json: String) -> bool` — deserialise an `IdbSchemaOp`
  from JSON, call `IdbBackend::apply_schema`, return success.
- Register `_lumen_idb_exec_op(json: String) -> String` — deserialise an `IdbRecordOp`,
  call `IdbBackend::exec_op`, return JSON-serialised `IdbOpResult`.
- Register `_lumen_idb_commit_txn(json: String) -> bool` — deserialise `Vec<IdbRecordOp>`,
  call `IdbBackend::commit_txn`, return success.
- Keep existing `_lumen_idb_load` / `_lumen_idb_persist` registrations unchanged (they
  remain the fallback path when `idb_backend` does not implement structured methods).

All primitives are registered only when `idb_backend` is `Some`, matching the existing guard
at `dom.rs:1664`.

### Step 4 — Wire JS shim to structured Rust primitives

Modify the JS section in `crates/js/src/dom.rs` starting around line 9452.

- In `_idb_flush_txn` (`dom.rs:9779`): after processing the request queue, if structured
  primitives are available (`typeof _lumen_idb_commit_txn === 'function'`), serialise the
  batch of record ops (adds, puts, deletes, clears from `txn._queue`) and call
  `_lumen_idb_commit_txn(json)` instead of setting `_idb_dirty = true`.
- In `_idb_process_open` (`dom.rs:10275`): if a `versionchange` transaction ran, call
  `_lumen_idb_schema_op(json)` after the upgrade transaction completes.
- On database open: call `_lumen_idb_load_version(name)` (new primitive) to check if the
  stored version matches the requested version, eliminating the need to deserialise the full
  snapshot just to check the version number.
- The blob path (`_idb_persist_if_dirty`) is kept as fallback: fires when structured
  primitives are absent.

### Step 5 — Shell integration

- In `crates/shell/src/main.rs`, change the `idb_backend` construction to use
  `NativeIdbStore::for_origin(etld_plus_one, idb_dir)` instead of `IdbStore::for_origin`.
- `etld_plus_one` is derived from the page URL the same way `ls_store_for_base` derives
  the origin key (`main.rs:4323`).
- Update `lumen_idb_dir()` (Step 0).
- Smoke-test: `cargo run -p lumen-shell -- samples/page.html` must open without errors.

### Step 6 — `databases()` API

`indexedDB.databases()` at `dom.rs:10372` currently returns an in-heap list. Wire it to
`IdbBackend::list_databases()` so it reflects persisted databases after a reload.

---

## Tests

All tests in `crates/storage/tests/` or `crates/storage/src/indexed_db.rs` (inline).

| Test | Validates |
|---|---|
| `native_idb_open_creates_tables` | Schema tables exist after `open_or_create` |
| `native_idb_version_persists` | `db_version()` reflects `apply_schema` bump |
| `native_idb_put_get_roundtrip` | `commit_txn([put])` + `exec_op(get)` |
| `native_idb_key_range_scan` | `exec_op(getAll with IDBKeyRange)` returns correct records |
| `native_idb_delete_record` | `commit_txn([delete])` removes the correct record |
| `native_idb_clear_store` | `commit_txn([clear])` empties a store |
| `native_idb_index_lookup` | Index entry retrieved via `json_extract` |
| `native_idb_multi_origin_isolation` | Two stores on different paths are independent |
| `native_idb_reload_survives` | Data written by one store instance is readable after `open_or_create` on same path |
| `native_idb_concurrent_read` | Two `open_or_create` on same path: reader sees writer's committed data |
| `native_idb_txn_abort_rolls_back` | Error in `commit_txn` → no partial writes |
| `native_idb_auto_increment` | `keyGenerator` persists across reloads |
| `native_idb_blob_fallback_compat` | `load()` / `save()` round-trip still works via synthetic snapshot key |
| JS integration (in `crates/js`) | `eval` a self-contained IDB script that opens, puts, reloads via `_lumen_idb_load`, gets |

---

## Definition of done

- [ ] `lumen_idb_dir()` returns `<exe>/data/idb/` (portable, no OS dirs).
- [ ] `NativeIdbStore` compiles and all unit tests pass.
- [ ] `clippy -p lumen-storage --all-targets -- -D warnings` clean.
- [ ] `clippy -p lumen-js --all-targets -- -D warnings` clean.
- [ ] `clippy -p lumen-shell --all-targets -- -D warnings` clean.
- [ ] `cargo test -p lumen-storage` — all new tests pass.
- [ ] JS integration test: open DB, write records, reload (call `_lumen_idb_load`), read
  records — values match.
- [ ] `cargo run -p lumen-shell -- samples/page.html` opens without panics.
- [ ] `CAPABILITIES.md` line for IndexedDB updated from partial to ✅.
- [ ] `docs/decisions/ADR-012-storage-partitioning.md` note updated: IDB per-origin file
  now uses portable `<exe>/data/idb/` path (was OS dirs).
