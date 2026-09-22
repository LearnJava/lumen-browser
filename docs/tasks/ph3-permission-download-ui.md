# Ph3 — Permission prompt UI + Download UI

**Developer:** P4 · **Branch:** `p4-ph3-permission-download-ui` · **Size:** M · **Crates:** `lumen-shell` (`panels/`, `download.rs`), light touch on `lumen-storage` (open the existing store) and `lumen-js` (request callback for prompts)

---

## Status

**Part B done (P1, 2026-09-22, `p1-ph3-downloadui`) — Part A (permission prompt UI) was already complete before this pass** (`ROADMAP.md`'s `P3-downloadui` line scopes this task to Part B only: *"Download UI over existing downloads storage (permission prompt UI already done)"*).

Part B closed the real gap: `crates/storage/src/downloads.rs`'s `Downloads` store already existed, fully tested, but nothing in `lumen-shell` ever opened or wrote through it — `DownloadManager` kept history in a plain `Vec` that reset on every restart. `DownloadManager::open_history(path)` (wired in `window_mode.rs` to `<exe_dir>/data/downloads.db`) now loads finished entries on startup and every state transition (`start_download`/`poll`/`cancel`) writes through to the store; a new header "Очистить" button drops finished entries from both the in-memory list and the store. Falls back to session-only history (not a panic) if the file can't be opened. Full writeup — `subsystems/shell.md`'s `ph3-permission-download-ui.md` Part B entry.

This is a UI-wiring task, **not** a from-scratch build. The storage layers exist; significant UI exists too. The honest gap **was**:

- **Permissions:** per `ROADMAP.md`'s `P3-downloadui` line, this half was already closed by an earlier (untracked-in-this-brief) pass before this session — not verified or touched here; if the panel/enum-unification/prompt gaps described below still exist, that is a doc-sync gap in `ROADMAP.md`, not this task's scope.
- **Downloads:** storage + panel + actions are largely complete and wired. The gap was narrow (history persistence + a "clear finished" affordance) — closed above.

---

## Goal

Make permissions and downloads *user-facing and durable*:

1. **Part A — Permission prompt UI.** When a page calls a permission-gated JS API (geolocation, notifications, camera/mic, clipboard), surface a real allow/deny **prompt** anchored to the page, persist the decision in the existing `lumen-storage` `Permissions` store, and have the JS API result follow the stored state. Unify the panel's duplicate enums with the storage layer's.
2. **Part B — Download UI polish.** The shelf already renders and acts; close the remaining gaps (persist download history across sessions, ensure the panel reflects all states correctly, expose a "clear finished" affordance).

---

## Current state (real file:line)

### Permissions — storage (COMPLETE)
- `crates/storage/src/permissions.rs:100` — `Permissions` struct (SQLite-backed).
- `crates/storage/src/permissions.rs:20` — `PermissionKind` enum: `Camera`, `Microphone`, `Geolocation`, `Notifications`, `Clipboard`, `Midi`, `PersistentStorage`, `Other(String)`.
- `crates/storage/src/permissions.rs:62` — `PermissionState` enum: `Prompt` / `Granted` / `Denied`.
- API: `set` (`:146`), `query` with expiry (`:170`), `touch` (`:199`), `revoke` (`:213`), `list_for_origin` (`:227`), `list_all` (`:249`), `clear_expired` (`:271`), `clear_origin` (`:286`).
- Exported from `crates/storage/src/lib.rs:81`.
- **Gap:** no `Permissions::open` call anywhere in `crates/shell` — the store is never instantiated. (grep `Permissions::open` in shell → 0 hits.)

### Permissions — panel (EXISTS, but disconnected)
- `crates/shell/src/panels/permission_panel.rs` — floating popover, toggled `Ctrl+Shift+P`.
- Wired into shell: field `permission` (`crates/shell/src/main.rs:5574`, init `:664`); hit-test (`main.rs:8894`); render `build_panel` (`main.rs:10370`).
- **Gap 1 — duplication:** the panel defines its *own* `PermissionKind` (`permission_panel.rs:56`, only 4 kinds) and `PermissionState` (`:99`, `Allow`/`Deny`/`Ask`) — distinct from the storage enums. These should be unified (re-export / map to `lumen_storage::PermissionKind` + `PermissionState`).
- **Gap 2 — no persistence:** state lives in `PermissionPanel.permissions: HashMap<(String, PermissionKind), PermissionState>` (`permission_panel.rs:143`); the module doc (`:6`) explicitly says "does not persist; StorageBackend hook-up is a future task". Cycle button mutates the map only (`:182`).
- **Gap 3 — viewer, not prompt:** the panel is a manual per-site toggle list. There is no *prompt* surfaced in response to an API call, and no Allow/Deny action that resolves a pending JS request.

### Permission-gated JS APIs (all stubbed, none prompt)
- `crates/js/src/dom.rs:5993` — `navigator.permissions.query` returns a hardcoded state from a static deny-list (`dom.rs:5988`); comment at `:5982` literally says "When P3 adds per-site permission UI the state values can be updated at runtime."
- `crates/js/src/geolocation.rs:43` — `install_geolocation_bindings`; always resolves with fake coords (`geolocation.rs:19`), `PERMISSION_DENIED` path exists (`:103`) but is never taken by a user decision.
- `crates/js/src/notifications_bindings.rs` — `Notification.requestPermission()` returns the shell-configured constant (`:90`, `granted`/`denied`); default `denied` (`:12`). No prompt; `_lumen_notification_request_permission` (`:88`).
- `crates/js/src/device_sensors.rs:26,44` — `requestPermission()` always resolves `'granted'` (`:4`).
- Media: `crates/js/src/media_devices.rs`, `media_capture.rs`, `screen_capture.rs` — getUserMedia / display-capture stubs.

### Downloads — storage + manager + panel (LARGELY COMPLETE)
- `crates/shell/src/download.rs:143` — `DownloadManager` (background threads + mpsc).
- `crates/shell/src/download.rs:71` — `DownloadEntry` (url/dest/filename/status/received/total).
- `crates/shell/src/download.rs:50` — `DownloadStatus`: `Pending` / `InProgress` / `Done{bytes}` / `Failed(String)` / `Cancelled`.
- Actions: `start_url_download` (`:269`), `cancel` (`:219`), `open_download` (`:236`), `show_in_folder` (`:250`), `poll` (`:283`).
- Panel: `build_download_bar` (`:751`), `hit_test` → `DownloadAction` Open/Reveal/Cancel/Close/Inside/Outside (`:721`, `:106`), progress bar + human-readable bytes (`:955`).
- Wired into shell: field `downloads` (`main.rs:5483`, init `:635`); `poll()` each tick (`main.rs:7618`); drains `_lumen_network_download` page requests and starts downloads (`main.rs:7620`); click hit-test (`main.rs:8509`); render `build_download_bar` (`main.rs:10247`). Toggle `Ctrl+Shift+J`.
- **Gap:** entries live only in `DownloadManager.entries: Vec<DownloadEntry>` (`download.rs:144`) — **no persistence**; history is lost on restart. There is no `Downloads` SQLite store in `crates/storage` (grep `download` in `crates/storage` → 0 hits). No "clear finished" control.

---

## Part A — Permission prompt UI

Turn the per-site viewer into a request-driven prompt backed by the storage layer.

1. **Open the store.** Instantiate `lumen_storage::Permissions` in the shell (alongside the other stores; portable data dir per `browser_data_dir()` convention — see CLAUDE.md "Known gotchas"). Add it to the shell state next to `permission` (`main.rs:5574`).
2. **Unify the enums.** Replace the panel's local `PermissionKind` (`permission_panel.rs:56`) and `PermissionState` (`:99`) with the storage enums (re-export or a thin shell-side mapping). Keep the panel's `cycle()`/labels/icons as presentation on top.
3. **Back the panel by the store.** `state_for` (`permission_panel.rs:169`) reads `Permissions::query(origin, kind, now)`; the toggle (`cycle_permission`, `:182`) writes via `Permissions::set(...)`. Drop the in-memory `HashMap`.
4. **Prompt surface (new).** Add a *prompt* mode to the panel (or a sibling overlay): when a pending permission request exists for the current origin, show the origin + kind + **Allow / Block** buttons. Allow → `set(Granted)`; Block → `set(Denied)`; remember-choice writes a permanent grant, otherwise a session/`expires_at` grant.
5. **JS → shell request channel (new).** Add a thread-local request queue in `lumen-js` (mirror `download_bindings::take_download_requests`, `main.rs:7624`) so a gated API can enqueue a `PermissionRequest { origin, kind }` and await the shell's decision. Wire `navigator.permissions.query` (`dom.rs:5993`), Notification (`notifications_bindings.rs:88`), and geolocation (`geolocation.rs`) to consult the stored state first and enqueue a prompt when state is `Prompt`.
   - *Scope guard:* full async round-trip to JS may be large. Minimum viable path: stored state drives the JS result on the **next** call; the first call enqueues a prompt and resolves with the default-deny while the user decides. Note the chosen approach in the commit body.

## Part B — Download UI (shelf/panel, progress, open/cancel)

The shelf, progress bar, and Open/Reveal/Cancel/Close actions already work (`download.rs:751`, `:721`). Remaining:

1. **Persist history (new).** Add a `Downloads` store in `crates/storage` (SQLite, mirror `permissions.rs` structure: id/url/dest/filename/status/bytes/created_at). On `poll()` completion (`download.rs:294`) upsert the entry; on startup load finished entries so the shelf shows history.
2. **Clear finished.** Add a "Clear" affordance to the header (next to the close × at `download.rs:677`) that drops `Done`/`Failed`/`Cancelled` entries (and their rows from the store).
3. **State coverage check.** Verify the shelf renders all five `DownloadStatus` variants correctly after a restart (currently only live entries are exercised by tests).

---

## Entry points (real file:line; *proposed* marked)

| Concern | Location | Note |
|---|---|---|
| Permissions store API | `crates/storage/src/permissions.rs:100` | exists |
| Open Permissions store in shell | `crates/shell/src/main.rs:~635` (near other store init) | **proposed** |
| Panel state field | `crates/shell/src/main.rs:5574` | exists |
| Panel `state_for` → store | `crates/shell/src/panels/permission_panel.rs:169` | **modify** |
| Panel toggle → `set()` | `crates/shell/src/panels/permission_panel.rs:182` | **modify** |
| Prompt overlay (Allow/Block) | `crates/shell/src/panels/permission_panel.rs` | **proposed** |
| JS permission request queue | `crates/js/src/` (mirror `download_bindings`) | **proposed** |
| Drain permission requests | `crates/shell/src/main.rs:~7636` (next to download drain) | **proposed** |
| `navigator.permissions.query` wiring | `crates/js/src/dom.rs:5993` | **modify** |
| Notification permission wiring | `crates/js/src/notifications_bindings.rs:88` | **modify** |
| Geolocation deny path | `crates/js/src/geolocation.rs:103` | **modify** |
| Download manager / poll | `crates/shell/src/download.rs:283` | exists |
| Downloads SQLite store | `crates/storage/src/downloads.rs` | **proposed** |
| Persist on completion | `crates/shell/src/download.rs:294` | **modify** |
| Clear-finished button | `crates/shell/src/download.rs:677` (header rects) | **proposed** |

---

## Steps

1. Reserve the task: branch `p4-ph3-permission-download-ui`, worktree under `.claude/worktrees/`, update `STATUS-P4.md` in the first commit.
2. **Part A.1–A.3:** open `Permissions` store in shell; unify panel enums with storage enums; back `state_for`/toggle with the store. Keep the existing `Ctrl+Shift+P` viewer working against persisted data.
3. **Part A.4–A.5:** add the prompt overlay + JS request queue; wire `navigator.permissions.query`, Notification, geolocation to the stored state and enqueue prompts on `Prompt`.
4. **Part B.1:** add `crates/storage/src/downloads.rs` + export; upsert on completion; load history on startup.
5. **Part B.2–B.3:** add clear-finished control; verify all status variants render post-restart.
6. Update docs in the same commits: `CAPABILITIES.md` (permissions UI + download history), `subsystems/storage.md` and `subsystems/shell.md`, `SYMBOLS.md` (`python scripts/gen_symbols.py`) for new public API.

## Tests

- `lumen-storage`: unit tests for the new `Downloads` store (insert/load/clear), mirroring `permissions.rs` tests (`permissions.rs:308`). The `Permissions` store already has thorough tests.
- `lumen-shell` `permission_panel`: extend existing tests (`permission_panel.rs:427`) — `state_for` reflects a persisted grant; toggle writes through to the store; prompt overlay emits Allow/Block hits.
- `lumen-shell` `download`: extend existing tests (`download.rs:983`) — history reloads finished entries on startup; clear-finished removes only terminal states; all five status variants render.
- `cargo clippy -p lumen-shell --all-targets -- -D warnings`, `cargo clippy -p lumen-storage --all-targets -- -D warnings`, then `cargo test -p lumen-storage` / `-p lumen-shell`.

## Definition of done

- [ ] A page calling a gated API on an origin with no stored decision produces a visible Allow/Block prompt; the decision persists and drives subsequent JS API results (`navigator.permissions.query`, Notification, geolocation) without re-prompting. — Part A, out of this pass's scope (see Status).
- [ ] The `Ctrl+Shift+P` panel reads and writes the `lumen-storage` `Permissions` store (survives restart); the panel's duplicate enums are removed in favour of the storage enums. — Part A, out of this pass's scope.
- [x] Download history survives a restart; the shelf shows finished entries; a clear-finished control works. (2026-09-22, P1)
- [x] `DownloadStatus` variant coverage: `Done`/`Failed`/`Cancelled`/`InProgress` all render distinctly (tests `build_bar_shows_filename`, `build_bar_shows_failed_and_cancelled_meta`, `build_bar_shows_progress_track_for_in_progress_only`). `Pending` remains unreachable through the public API today (`start_download` sets `InProgress` directly; storage-level "pending" only ever means "no live thread to resume", handled by `load_history` dropping it) — pre-existing, not part of this gap.
- [x] New public API indexed in `SYMBOLS.md` (`python scripts/gen_symbols.py`); `CAPABILITIES.md` and `subsystems/shell.md` updated in the same commit; `cargo clippy -p lumen-shell --all-targets -- -D warnings` clean; `lumen-shell` suite (2077 tests, +7 new) green.
