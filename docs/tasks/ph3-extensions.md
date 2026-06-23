# Ph3 — Extensions (minimal native format)

**Developer:** P1  
**Branch:** `p1-ph3-extensions`  
**Size:** XL  
**Crates:** `lumen-shell` (extensions/), `lumen-js`, `lumen-storage`  
**Phase:** 3 (v1.0 target)

---

## Status

Phase 3 future item. Partial scaffolding exists: Phase 0 of task D-6 is already
merged into main. The loader, URL pattern matcher, and content-script injection
seam are live. The JS-side `chrome.runtime` stub is live. What is missing is the
native manifest format, permission enforcement, request-filter hooks, background
script execution, message-passing IPC, UI surface extension points, and portable
storage.

---

## Goal

A minimal **native** extension format — not WebExtensions/Chrome compatibility.
Lumen extensions are small Rust-friendly packages that can:

1. Inject JS into matching pages (content scripts) — **already works partially**
2. Intercept and block/redirect network requests (request filters)
3. Persist private data per extension (storage)
4. Optionally add a toolbar button and a popup HTML panel (UI surface)

Design principles:
- **No native code in extensions** — JS only for content scripts + background.
- **Explicit permissions** — each capability declared in manifest and enforced by Rust.
- **Portable data** — stored under `<exe_dir>/data/extensions/<id>/` (same convention as ad-block; never OS dirs).
- **No WebExtensions/Chrome API compatibility guarantee** — the `chrome.runtime.sendMessage` stub is kept so existing content-script snippets don't throw, but full WebExtensions fidelity is not a goal.

---

## Current state

### What exists: `crates/shell/src/extensions/mod.rs`

**File: `crates/shell/src/extensions/mod.rs`**

| Symbol | Line | Status |
|---|---|---|
| `ExtensionManifest` struct | 42 | Live — `name`, `version`, `permissions: Vec<String>`, `content_scripts` |
| `ContentScript` struct | 33 | Live — `matches: Vec<String>`, `js: Vec<String>` |
| `ExtensionRegistry` struct | 69 | Live — `Vec<LoadedExtension>` |
| `ExtensionRegistry::load()` | 99 | Live — scans `extensions_dir()` at startup |
| `ExtensionRegistry::load_from_dir()` | 108 | Live — used in tests |
| `ExtensionRegistry::content_scripts_for_url()` | 151 | Live — returns `Vec<String>` of JS source |
| `extensions_dir()` | 80 | Live — `%APPDATA%\lumen\extensions\` (Windows) / `$XDG_CONFIG_HOME/lumen/extensions/` |
| `url_matches()` | 316 | Live — Chrome-style glob patterns, `<all_urls>`, `*.example.com` |
| `parse_manifest()` | 175 | Live — hand-rolled JSON scanner; no serde |

**Gap:** `permissions` field is parsed but **never enforced** (comment at line 49:
`#[allow(dead_code)]`). No background scripts, no request-filter hooks, no
UI surfaces, no per-extension storage. Storage path uses OS config dir
(`%APPDATA%`) rather than portable `<exe_dir>/data/` (CLAUDE.md §Known gotchas).

### Content-script injection seam: `crates/shell/src/main.rs`

**File: `crates/shell/src/main.rs`**

| Location | Line (approx) | Description |
|---|---|---|
| `let ext_registry = extensions::ExtensionRegistry::load()` | 3746 | Registry instantiated per page load |
| `let ext_scripts = ext_registry.content_scripts_for_url(&page_url)` | 3747 | JS sources collected |
| `extra_scripts: &ext_scripts` passed to `run_scripts_with_dom()` | 3775 | Sources threaded into script runner |
| `for src in extra_scripts { rt.eval(src) }` | 4976 | Content scripts evaluated after all page scripts |

**Gap:** `ExtensionRegistry::load()` is called on **every page load** — no caching.
Content scripts run after page scripts, which is correct per Chrome spec. No
way to run scripts at `document_start` (before DOM is built).

### JS-side `chrome.runtime` stub: `crates/js/src/dom.rs`

**File: `crates/js/src/dom.rs`**

| Location | Line (approx) | Description |
|---|---|---|
| Native binding `_lumen_chrome_runtime_send_message` | 2493–2498 | No-op; logs to stderr in Phase 0 |
| JS stub block (IIFE) | 11820–11852 | Installs `chrome.runtime` / `browser.runtime` when `_LUMEN_EXTENSION_ACTIVE` global is set |
| `sendMessage(msg, callback)` | 11829 | Calls native binding; callback receives `undefined` |
| `onMessage.addListener / removeListener / hasListener` | 11833–11840 | Listener list maintained in JS; never dispatched to |
| `getURL(path)` | 11841 | Returns `chrome-extension://lumen-extension/<path>` |
| `getManifest()` | 11842 | Returns stub `{name:'', version:'0', manifest_version:3}` |
| Guard `_LUMEN_EXTENSION_ACTIVE` | 11826 | Avoids contaminating CDP automation-detection checks |

**Gap:** `_LUMEN_EXTENSION_ACTIVE` is never set in production — only in unit tests
(line 11936). `onMessage` listeners are never dispatched to. No way for a content
script to receive a reply from a background script.

### Ad-block precedent for request filtering

**File: `crates/shell/src/adblock.rs`**

The ad-block subsystem demonstrates the full pattern extensions' request filters
should follow:

- `browser_data_dir()` at line 44 — `<exe_dir>/data/` portable root
- `adblock_dir()` at line 52 — `<data>/adblock/`
- Filter installed via `install_global_adblock_filter()` from `lumen-network`
- Hot path: `RequestFilter::should_block_ctx()` trait in `crates/core/src/ext.rs:116`
- `EasyListFilter` is an `Arc<dyn RequestFilter>` passed to `HttpClient::with_filter()`

Extension request filters should follow the same `RequestFilter` trait path:
`ExtensionRequestFilter` implements `RequestFilter`, gets wrapped in
`CompositeFilter` alongside ad-block, installed via `install_global_adblock_filter`
analogue.

---

## Architecture

### Manifest schema (proposed native format)

Replace the current Chrome MV3 subset with a Lumen-native TOML manifest
(avoids the hand-rolled JSON scanner; aligns with other Lumen config files):

```toml
# <ext_dir>/manifest.toml
name    = "My Extension"
version = "1.0.0"
id      = "my-extension"          # stable slug, filesystem-safe

[permissions]
content_scripts = true
request_filter  = false
storage         = false
ui_panel        = false

[[content_scripts]]
matches    = ["https://example.com/*"]
js         = ["content.js"]
run_at     = "document_end"        # document_start | document_end (default)

[background]
js = "background.js"               # optional; runs in a persistent JS context

[ui]
toolbar_icon = "icons/32.png"      # optional; adds a toolbar button
popup        = "popup.html"        # optional; HTML panel opened by toolbar button
```

**Backward compatibility:** the existing JSON `manifest.json` loader
(`parse_manifest()` in `extensions/mod.rs:175`) continues to work as a fallback
for Phase 0 extensions. New extensions use TOML. The `ExtensionManifest` struct
gains new fields for background/ui/run_at.

### Capability and permission model

Each capability maps to one manifest key and one Rust enforcement point:

| Capability | Manifest key | Enforcement |
|---|---|---|
| Content scripts | `permissions.content_scripts = true` | `ExtensionRegistry::content_scripts_for_url()` — only called when permission granted |
| Request filter | `permissions.request_filter = true` | Extension's filter added to `CompositeFilter` only when granted |
| Storage | `permissions.storage = true` | `ExtensionStore::open()` — returns `Err` when not granted |
| UI panel | `permissions.ui_panel = true` | Toolbar button rendered only when granted |

Permissions are declared in the manifest and verified at load time (not at runtime).
Users install extensions manually (no browser extension store in Phase 3).
No runtime permission-grant UI in Phase 3 — all-or-nothing install.

### Content scripts

**Run-at timing:**

- `document_start` — inject before any page scripts; requires a new injection
  hook at the start of `run_scripts_with_dom()` (currently only `document_end` is
  wired via the `extra_scripts` parameter at
  `crates/shell/src/main.rs:4907`).
- `document_end` (default) — current behavior: runs after all page scripts
  (`crates/shell/src/main.rs:4975–4986`).

**`_LUMEN_EXTENSION_ACTIVE` guard:** must be set before `install_dom()` when any
extension has `content_scripts` permission. Currently only set in tests
(`crates/js/src/dom.rs:11936`). Production path: set in `run_scripts_with_dom()`
when `extra_scripts` is non-empty.

**Registry caching:** `ExtensionRegistry::load()` is called per page load
(line 3746). Promote to a once-loaded registry stored in `BrowserState`
(or `Arc<ExtensionRegistry>`) and reloaded only on explicit user request.

### Background scripts (proposed)

A background script runs in a persistent `QuickJsRuntime` that lives for the
browser session (not per-page). It receives messages from content scripts via
`chrome.runtime.sendMessage`.

- **Storage:** one `QuickJsRuntime` per extension with `background.js` declared.
  Stored in `ExtensionRegistry` as `Vec<BackgroundContext>`.
- **Message bus:** when a content script calls `_lumen_chrome_runtime_send_message`,
  the native binding (currently a no-op at `crates/js/src/dom.rs:2496`) looks up
  the background context for the active extension and calls `eval_js()` on it with
  a dispatch call.
- **Lifetime:** background context created at browser start alongside ad-block
  init; not per-tab. Destroyed on browser exit.

### Request-filter hooks (proposed)

Extension request filters hook into the same `RequestFilter` trait path as ad-block:

```rust
// crates/core/src/ext.rs:116 — existing trait
pub trait RequestFilter: Send + Sync {
    fn should_block(&self, url: &Url) -> Option<String>;
    fn should_block_ctx(&self, url: &Url, ctx: &RequestContext) -> Option<String>;
}
```

Implementation plan:
1. Add `ExtensionRequestFilter` in `crates/shell/src/extensions/mod.rs` that
   holds parsed block/allow rules from each extension's background script
   (or a static `declarativeNetRequest` section in the manifest — Phase 3.1+).
2. At browser startup, wrap it in `CompositeFilter` alongside `EasyListFilter`.
3. Install via the same `install_global_adblock_filter()` call site
   (`crates/shell/src/main.rs` startup block).

Phase 3.0: static rule list in manifest (`[[request_rules]]` TOML). Background
script dynamic rules are Phase 3.1.

### UI surfaces (proposed)

Toolbar button + popup panel:

- `[ui] toolbar_icon` — a PNG icon rendered in the tab strip / chrome area.
  Extension toolbar buttons are collected in a `Vec<ExtensionUiEntry>` at load
  time and drawn by the shell's panel renderer
  (`crates/shell/src/panels/mod.rs`).
- `[ui] popup` — an HTML file from the extension directory. Opening the toolbar
  button navigates a floating panel (`pip_window.rs` pattern) to a local
  `lumen-extension://<id>/popup.html` URL.
- `lumen-extension://` scheme: a new URL scheme resolved by the shell network
  layer that reads files from the extension directory, similar to `file://` but
  scoped to the extension.

Phase 3.0: icon rendering + popup panel. No content_script ↔ popup messaging in
Phase 3.0 (requires background script IPC).

### Install / storage / loading (proposed)

**Loading:**

Extensions are loaded at browser startup from the extensions directory. Two
storage locations:

1. **OS config dir** (current behavior for manifest lookup):
   `%APPDATA%\lumen\extensions\` (Windows) — `extensions_dir()` at
   `crates/shell/src/extensions/mod.rs:80`.
2. **Portable data dir** (for extension private data — CLAUDE.md §Known gotchas):
   `<exe_dir>/data/extensions/<id>/` — analogous to `adblock_dir()`.

These are different directories with different purposes:
- Config dir: extension source files (manifest + JS + icons) — user-installed.
- Data dir: extension private key-value storage (localStorage equivalent).

**`ExtensionStore` (proposed, in `crates/storage/src/`):**

A new `extensions.rs` in `lumen-storage` following the `adblock.rs` pattern:
- SQLite file at `<data>/extensions/<id>/storage.db`.
- API: `get(key) -> Option<String>`, `set(key, value)`, `remove(key)`, `clear()`.
- Exposed to JS via `chrome.storage.local` (background + content scripts).

---

## Entry points

| File | Line | Symbol | Status |
|---|---|---|---|
| `crates/shell/src/extensions/mod.rs` | 1 | entire module | **live** — Phase 0 |
| `crates/shell/src/extensions/mod.rs` | 42 | `ExtensionManifest` | **live** — extend with `background`, `ui`, `run_at` fields |
| `crates/shell/src/extensions/mod.rs` | 69 | `ExtensionRegistry` | **live** — add caching, background contexts, request filters |
| `crates/shell/src/extensions/mod.rs` | 80 | `extensions_dir()` | **live** — OS config dir; keep for source files |
| `crates/shell/src/main.rs` | 3746 | `ExtensionRegistry::load()` call | **live** — move to startup; cache in `BrowserState` |
| `crates/shell/src/main.rs` | 3775 | `extra_scripts` param | **live** — add `document_start` branch |
| `crates/shell/src/main.rs` | 4907 | `extra_scripts: &[String]` param in `run_scripts_with_dom` | **live** — add `start_scripts: &[String]` parallel param |
| `crates/shell/src/main.rs` | 4975 | `for src in extra_scripts` loop | **live** — add symmetric loop at top of function for `document_start` |
| `crates/js/src/dom.rs` | 2493 | `_lumen_chrome_runtime_send_message` native binding | **live stub** — wire to background context dispatch |
| `crates/js/src/dom.rs` | 11826 | `_LUMEN_EXTENSION_ACTIVE` guard | **live** — set in production when ext active |
| `crates/core/src/ext.rs` | 116 | `RequestFilter` trait | **live** — reuse for extension request filters |
| `crates/shell/src/adblock.rs` | 44 | `browser_data_dir()` | **live** — reuse for extension storage path |
| `crates/storage/src/extensions.rs` | — | `ExtensionStore` | **proposed** — new file |
| `crates/shell/src/extensions/request_filter.rs` | — | `ExtensionRequestFilter` | **proposed** — new file |
| `crates/shell/src/extensions/background.rs` | — | `BackgroundContext` | **proposed** — new file |

---

## Steps

### Step 1: Migrate storage dir to portable path

**File:** `crates/shell/src/extensions/mod.rs:80` — `extensions_dir()`

Change from OS config dir to `<exe_dir>/data/extensions/` using
`browser_data_dir()` from `crates/shell/src/adblock.rs:44`. Add a symlink/copy
migration path for users who had extensions installed in the old location.

> **Note:** This is a breaking change for existing Phase 0 installations.
> Document in a migration note; extension count in Phase 0 is 0 real users.

### Step 2: TOML manifest support

**File:** `crates/shell/src/extensions/mod.rs`

Add `parse_manifest_toml(toml_str: &str) -> Option<ExtensionManifest>` using
the existing `toml` crate (already a workspace dependency via `lumen-shell`'s
`Cargo.toml`). Extend `ExtensionManifest` with:

```rust
pub struct ExtensionManifest {
    // existing fields ...
    pub id: String,                          // stable slug
    pub background_js: Option<String>,       // background.js filename
    pub toolbar_icon: Option<String>,        // toolbar icon path
    pub popup_html: Option<String>,          // popup HTML path
    pub run_at: RunAt,                       // document_start | document_end
}

#[derive(Debug, Clone, Default)]
pub enum RunAt {
    DocumentStart,
    #[default]
    DocumentEnd,
}
```

Keep `parse_manifest()` (JSON) working as fallback. `load_from_dir()` tries TOML
first (`manifest.toml`), then JSON (`manifest.json`).

### Step 3: Cache registry in BrowserState

**File:** `crates/shell/src/main.rs` ~line 3746

Move `ExtensionRegistry::load()` from the per-page-load pipeline into the browser
startup block (where `AdblockOrchestrator` is initialized). Store as
`Arc<ExtensionRegistry>` in `BrowserState`. Thread it into `run_scripts_with_dom`
via the existing call chain. Eliminates ~1 directory scan per navigation.

### Step 4: Set `_LUMEN_EXTENSION_ACTIVE` in production

**File:** `crates/shell/src/main.rs` ~line 3760 (inside `run_scripts_with_dom` call setup)

When any extension has content scripts matching the current URL, set the global
before `install_dom()` runs:

```rust
// Proposed: set before install_dom
if !ext_scripts.is_empty() {
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").ok();
}
```

### Step 5: `document_start` injection

**File:** `crates/shell/src/main.rs` — `run_scripts_with_dom()` signature (~line 4907)

Add `start_scripts: &[String]` parameter parallel to `extra_scripts`. Evaluate
`start_scripts` immediately after `install_dom()` but before any page scripts.

### Step 6: Permission enforcement

**File:** `crates/shell/src/extensions/mod.rs`

In `content_scripts_for_url()`, check `manifest.permissions.content_scripts`
before returning scripts. Add a `has_permission(cap: ExtensionCapability) -> bool`
helper on `ExtensionManifest`.

### Step 7: `ExtensionStore` in lumen-storage

**New file:** `crates/storage/src/extensions.rs`

SQLite-backed key-value store per extension. Pattern: copy `adblock.rs` structure.
Expose `get/set/remove/clear`. Wire `chrome.storage.local` JS binding in
`crates/js/src/dom.rs` (next to the `chrome.runtime` block at line 11820).

### Step 8: Background script context

**New file:** `crates/shell/src/extensions/background.rs`

One `QuickJsRuntime` per extension with `background_js` set. Created at browser
startup. `eval(src)` runs the background script. Wire
`_lumen_chrome_runtime_send_message` (currently no-op at `crates/js/src/dom.rs:2496`)
to dispatch a message into the matching background runtime.

### Step 9: Request filter hook

**New file:** `crates/shell/src/extensions/request_filter.rs`

`ExtensionRequestFilter` implementing `lumen_core::ext::RequestFilter`. Reads
static block/allow rules from `[[request_rules]]` in the TOML manifest. Wrapped
in `CompositeFilter` alongside `EasyListFilter` at startup.

### Step 10: UI surface (toolbar button + popup)

**Files:** `crates/shell/src/panels/mod.rs`, `crates/shell/src/main.rs`

Render toolbar icons for extensions with `ui_panel` permission. On click, open
a floating `pip_window.rs`-style panel navigated to `lumen-extension://<id>/popup.html`.
Add `lumen-extension://` scheme handler in the shell network layer (resolves files
from `<data>/extensions/<id>/` — the portable data dir).

---

## Security and privacy notes

- **No native code.** Extensions are JS + static assets only. Rust execution from
  extension code is not possible.
- **Explicit permission model.** Each capability is declared and enforced by Rust
  before any JS runs. A content-script-only extension cannot register a request filter.
- **Isolation.** Content scripts run in the same QuickJS realm as the page — no
  separate V8 isolate in Phase 3. Full sandbox isolation is Phase 4+.
- **`_LUMEN_EXTENSION_ACTIVE` guard** prevents `chrome.runtime` from being present
  on pages without extensions (avoids contributing to CDP fingerprinting surface;
  comment at `crates/js/src/dom.rs:11824`).
- **Storage isolation.** Each extension's SQLite file is under its own `<id>/`
  subdirectory. No cross-extension storage access.
- **`lumen-extension://` scheme** is origin-isolated: a content script cannot
  read another extension's popup HTML via `fetch("lumen-extension://other-ext/...")`.
- **Request filter rules** are static TOML in Phase 3.0. No dynamic rule updates
  from content scripts (prevents drive-by ad-block poisoning).

---

## Tests

| Test | Location | Type |
|---|---|---|
| `parse_manifest_toml_basic` | `extensions/mod.rs` | unit |
| `parse_manifest_toml_with_background` | `extensions/mod.rs` | unit |
| `parse_manifest_toml_fallback_to_json` | `extensions/mod.rs` | unit |
| `permission_enforcement_blocks_unpermitted_script` | `extensions/mod.rs` | unit |
| `run_at_document_start_executes_before_page_scripts` | `shell/src/main.rs` tests | integration |
| `extension_store_get_set_roundtrip` | `storage/src/extensions.rs` | unit |
| `extension_request_filter_blocks_matching_url` | `extensions/request_filter.rs` | unit |
| `chrome_runtime_send_message_dispatches_to_background` | `js/src/dom.rs` | unit |
| `extension_registry_cached_across_navigations` | `shell/src/main.rs` | unit |

Existing tests to keep passing:
- `crates/shell/src/extensions/mod.rs` — all 8 existing tests (lines 431–533)
- `crates/js/src/dom.rs` — `chrome_runtime_*` tests (lines 24183–24232)

---

## Definition of done

- [ ] TOML manifest parsed; JSON manifest still works as fallback
- [ ] Storage dir migrated to `<exe_dir>/data/extensions/` (portable)
- [ ] `ExtensionRegistry` cached at browser startup (not per navigation)
- [ ] `_LUMEN_EXTENSION_ACTIVE` set in production when extensions are active
- [ ] `document_start` injection path wired
- [ ] Permission enforcement: content_scripts / request_filter / storage / ui_panel gated
- [ ] `ExtensionStore` (`chrome.storage.local`) implemented and tested
- [ ] Background script context created at startup; `sendMessage` dispatches to it
- [ ] Static request-filter rules from manifest wired into `CompositeFilter`
- [ ] Toolbar button + popup panel rendered for extensions with `ui_panel` permission
- [ ] `lumen-extension://` scheme resolves extension assets
- [ ] All existing extension tests pass
- [ ] `cargo clippy -p lumen-shell --all-targets -- -D warnings` clean
- [ ] CAPABILITIES.md updated (Extensions row)
- [ ] `docs/plan/phases.md` Phase 3 checklist updated
