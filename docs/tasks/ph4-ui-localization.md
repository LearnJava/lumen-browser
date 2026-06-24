# Ph4 — UI localization

**Developer:** P1  
**Branch:** `p1-ph4-ui-localization`  
**Size:** L (mechanical breadth across every panel, no new algorithmic complexity)  
**Crates:** `lumen-shell`, `lumen-storage` (language setting)

---

## Status

**Phase 4 — after 1.0. Do not start until Phase 3 is complete.**

Listed in `docs/plan/phases.md:146` under "Phase 4 — After 1.0":

> Локализация UI.

The underlying approach is fixed in `docs/plan/features.md:58–60` (§10.8):

> Русский — первый язык наравне с английским, не «после релиза». Формат **Fluent** (FTL, Mozilla) — корректная плюрализация, грамматические падежи. Дизайн UI учитывает: русский текст в среднем на ~30% длиннее английского.

---

## Goal

Externalize every hard-coded UI string in `lumen-shell` into per-locale **Fluent** (`.ftl`) catalogs. Add a `ui_language` setting persisted in `BrowserSettings`. At startup and on settings change, load the matching catalog and look up every label via a `t(key)` call. Provide `en` and `ru` catalogs from day one; add further languages by shipping new `.ftl` files.

After this task:
- Users can switch the browser chrome language from Settings → Appearance.
- A Russian speaker sees "Загрузки", "Общие", "Закладки", "Назад" etc. without any English fallthrough.
- A new language can be added by adding a single `.ftl` file with no Rust changes.

---

## Current state

### How UI strings are written today

All UI text in `lumen-shell` is **inline string literals** — `&'static str` or owned `String` values returned directly from rendering functions. Representative examples:

| File | Line(s) | Strings |
|---|---|---|
| `crates/shell/src/panels/settings_panel.rs` | 87–90 | `"Общие"`, `"Конфиденц."`, `"Вид"`, `"Загрузки"` |
| `crates/shell/src/panels/command_palette.rs` | 113–131 | `"New Tab"`, `"Close Tab"`, `"Back"`, `"Forward"`, `"Find on Page"`, `"Toggle Bookmarks"`, … (14 action labels) |
| `crates/shell/src/panels/bookmark_panel.rs` | 327, 418, 463 | `"Bookmarks"`, `"No bookmarks"`, `"×"` |
| `crates/shell/src/panels/history_panel.rs` | 326, 365, 378, 512, 581, 584 | `"History"`, `"Search history…"`, `"No browsing history yet."`, `"Очистить всё"`, `"Today"`, `"Yesterday"` |
| `crates/shell/src/panels/shortcuts_panel.rs` | 79–98, 252 | `"Перезагрузить"`, `"Назад"`, `"Вперёд"`, `"Горячие клавиши"`, … |
| `crates/shell/src/panels/a11y_panel.rs` | 258, 284, 304, 318, 332, 398–400 | `"Accessibility"`, `"Font size"`, `"Reduced motion"`, `"Normal"`, `"Large"`, `"Extra"` |
| `crates/shell/src/panels/ai_panel.rs` | 207, 244, 307 | `"AI Assistant"`, `"Ask anything…"`, `"type a prompt…"` |
| `crates/shell/src/panels/cert_panel.rs` | 187–194, 261, 320 | `"Subject CN"`, `"Issuer CN"`, `"Valid From"`, `"Certificate Information"`, `"No certificate information …"` |
| `crates/shell/src/panels/privacy_panel.rs` | 246, 303, 355, 357 | `"Privacy"`, `"No requests yet"`, `"Disable for this site"`, `"Enable for this site"` |
| `crates/shell/src/panels/shields_panel.rs` | 289, 355, 357 | `"SHIELDS ON"`, `"SHIELDS OFF"`, `"Disable for this site"` |
| `crates/shell/src/panels/print_panel.rs` | 435, 484–508, 585–606, 622, 638 | `"Печать"`, `"Letter"`, `"Портрет"`, `"Альбом"`, `"Обычные"`, `"Цветной"`, `"Серый"`, `"Отмена"` |
| `crates/shell/src/download.rs` | 696, 700, 706, 776 | `"Открыть"`, `"Папка"`, `"Отмена"`, `"Загрузки ({active} активных)"` |
| `crates/shell/src/panels/permission_panel.rs` | 79–116 | `"Camera"`, `"Microphone"`, `"Notifications"`, `"Clipboard"`, `"Allow"`, `"Deny"`, `"Ask"` |
| `crates/shell/src/panels/read_later_panel.rs` | 187, 209, 248–250 | `"Read Later"`, `"No saved pages yet."`, `"Unread"`, `"Read"`, `"Archived"` |
| `crates/shell/src/panels/note_viewer.rs` | 190, 290 | `"Заметка"`, `"Комментарий:"` |
| `crates/shell/src/panels/focus_panel.rs` | 326–330 | `"Done"`, `"Focus"`, `"Paused"` |
| `crates/shell/src/panels/sidebar_panel.rs` | 245, 301, 316 | `"Sidebar"`, `"No page"`, `"Loading…"` |
| `crates/shell/src/address_bar.rs` | 214–221, 415 | `"история"`, `"заметка"`, `"запрос"`, `"вкладка"`, `"Введите URL или поисковый запрос…"` |
| `crates/shell/src/devtools/inspector.rs` | 619, 635, 643 | `"Inspector  (Ctrl+Shift+I)"`, `"Elements"`, `"Styles"` |
| `crates/shell/src/devtools/console_panel.rs` | 222, 232, 246, 284 | `"Console ({} messages)"`, `"F12 to close"`, `"Clear"`, `"↑↓  {}/{}"` |
| `crates/shell/src/devtools/network_panel.rs` | 431, 441, 452, 550 | `"Network ({} requests)"`, `"Ctrl+Shift+E to close"`, `"(no requests yet)"`, `"blocked"` |
| `crates/shell/src/newtab.rs` | 101 | `"История пуста — открытые страницы появятся здесь."` |
| `crates/shell/src/panels/workspace_panel.rs` | (color name constants) | workspace label strings |

The strings are currently **mixed-language**: many panels are in Russian (print, download, shortcuts, note, address bar hints), others in English (a11y, permissions, read later, bookmarks, history). This is not a language bug — it is the pre-localization state. The task's first job is to unify all strings through the catalog regardless of current language.

### Settings storage

`crates/storage/src/browser_settings.rs` — a SQLite key-value table. Current keys (lines 19–27): `homepage`, `search_engine_id`, `shields_enabled`, `fingerprint_mode`, `doh_enabled`, `font_size`, `theme`, `download_path`, `tab_layout`. No `ui_language` key yet.

`BrowserSettingsSnapshot` (`browser_settings.rs:43`) holds the typed snapshot passed to the settings panel. No language field.

Settings panel sections: `settings_panel.rs:63–91` — `General`, `Privacy`, `Appearance`, `Downloads`. Language picker belongs in **Appearance**.

### Existing i18n infrastructure

- `crates/js/src/intl_bindings.rs` — ECMA-402 `Intl` shim (JS side). Covers `en-US` and `ru-RU` for web pages. **Not usable for UI strings** — it lives in the JS engine, not the shell rendering layer.
- `crates/engine/encoding/` uses `icu_segmenter = "2"` (line 12 of its `Cargo.toml`). `icu_locale`, `icu_locale_core`, `icu_provider` are already in `Cargo.lock` as transitive deps of `icu_segmenter`. No Fluent crate is present anywhere in the workspace.
- `crates/shell/src/config.rs:116,154` — `languages: Vec<String>` in `BrowserConfig` holds `Accept-Language` values for HTTP requests. This is **not** a UI language setting; it controls what language web pages are served in.

### RTL

No RTL layout support in the shell renderer today. All panels assume left-to-right column/row layout hardcoded in femtovg draw calls. Arabic and Hebrew UI locales require right-mirroring panel layouts. This is a substantial follow-on.

---

## Architecture

### String catalog format: Fluent (FTL)

`docs/plan/features.md:60` mandates **Fluent** as the catalog format. Reasons:

1. Handles grammatical gender and plural forms correctly for Russian (1 файл / 2 файла / 5 файлов), German, etc. A simple `key → String` map cannot express this without baking locale logic into call sites.
2. Standard in browser development (Firefox, Thunderbird). Well-specified; tooling exists.
3. The `fluent` crate on crates.io (`fluent = "0.16"`) is a pure-Rust implementation with no heavy dependencies.

**Against a hand-rolled key→string map:** a map works for English-only but breaks on Russian pluralization. The `t("downloads-panel-title", count=3)` call must yield `"Загрузки (3 активных)"` — Fluent handles this natively. A map would require caller-side if/else per language, defeating the purpose.

**Catalog locations** (proposed): `assets/locales/{lang}/ui.ftl`, e.g. `assets/locales/en/ui.ftl`, `assets/locales/ru/ui.ftl`. Embedded at compile time via `include_str!` or `include_bytes!`.

### Runtime locale resolution

```
1. Read `ui_language` from BrowserSettings  →  e.g. "ru"
2. If not set or empty: fall back to OS locale (std::env::var("LANG"), registry on Windows)
3. If OS locale unrecognised: fall back to "en"
4. Load catalog:  assets/locales/{lang}/ui.ftl
5. If key missing in chosen locale: fall back to "en" catalog
```

### `t(key)` lookup interface (proposed, in `lumen-shell`)

A session-global `Locale` struct (one per browser window, re-created on language change) holds the parsed `FluentBundle`. A single function `t(key: &str) -> String` (and `tn(key: &str, count: u64) -> String` for plurals) replaces every inline literal. All panel render functions receive `&Locale` (or access it via the existing `Palette`-passing pattern).

### Panels to touch

Every panel listed in **Current state** above. The mechanical pattern at each site:

```
// Before:
text: "No bookmarks".to_owned()

// After:
text: locale.t("bookmarks-empty-label")
```

Parameterized strings:

```
// Before (download.rs:776):
format!("Загрузки ({active} активных)")

// After:
locale.tn("downloads-panel-title", active as u64)
// FTL: downloads-panel-title = { $count ->
//   [one]   Загрузки ({$count} активная)
//   [few]   Загрузки ({$count} активных)
//  *[other] Загрузки ({$count} активных)
// }
```

### Number and date formatting

- Dates in history panel (`history_panel.rs:568–605`) use `dd.mm.yyyy` hardcoded for `ru` locale and ISO for `en`. After this task: use `Locale::format_date(ts)` which dispatches per locale.
- Numbers: `Locale::format_count(n)` wraps locale-aware thousands separator (space for Russian, comma for English). Uses the existing `Intl` shim logic as a reference; no new heavy dep needed for the small set of formats required by shell UI.

### RTL note

RTL UI layout (Arabic, Hebrew, Persian) requires mirroring the panel column order and tab strip direction in femtovg draw calls. This is a larger geometric change touching every panel's coordinate arithmetic. **Not in scope for this task.** File as a Phase 4 follow-on if an RTL locale is ever requested.

---

## Entry points

All entries are **proposed** (no localization infrastructure exists today).

| # | File | Line | What to change |
|---|---|---|---|
| 1 | `crates/storage/src/browser_settings.rs` | 27 (after `KEY_TAB_LAYOUT`) | Add `const KEY_UI_LANGUAGE: &str = "ui_language";` + getter/setter + field in `BrowserSettingsSnapshot` |
| 2 | `crates/shell/src/panels/settings_panel.rs` | 63–92 (`SettingsSection`) | Add language picker row in Appearance section |
| 3 | `crates/shell/src/` (new file) | — | `locale.rs`: `Locale` struct, `FluentBundle` init, `t(key)`, `tn(key, count)`, `format_date(ts)`, `format_count(n)` |
| 4 | `assets/locales/en/ui.ftl` | — | New file: English catalog (all keys) |
| 5 | `assets/locales/ru/ui.ftl` | — | New file: Russian catalog (all keys) |
| 6 | Every panel file listed in **Current state** | (lines above) | Replace inline literal with `locale.t("key")` |
| 7 | `crates/shell/src/main.rs` | startup | Read `ui_language` from `BrowserSettings`, construct `Locale`, thread it to panels |

---

## Steps

### Step 1 — Infra: storage + Locale struct

1. `browser_settings.rs`: add `KEY_UI_LANGUAGE`, `DEFAULT_UI_LANGUAGE = ""` (empty = OS detection), `ui_language()`, `set_ui_language()`, field `ui_language: String` in `BrowserSettingsSnapshot`, wire into `snapshot()` and `apply_snapshot()`.
2. Add `fluent = "0.16"` to `crates/shell/Cargo.toml`. Justify in commit body: category = provisional; fluent is the format mandated by features.md §10.8; graduation criterion = when a third language ships.
3. Create `crates/shell/src/locale.rs`:
   - `pub struct Locale { bundle: FluentBundle<FluentResource> }`
   - `pub fn Locale::load(lang_tag: &str) -> Self` — loads `assets/locales/{lang}/ui.ftl`, falls back to `en`.
   - `pub fn t(&self, key: &str) -> String`
   - `pub fn tn(&self, key: &str, count: u64) -> String`
   - `pub fn format_date(&self, unix_secs: i64) -> String`
   - `pub fn format_count(&self, n: u64) -> String`
4. Create `assets/locales/en/ui.ftl` and `assets/locales/ru/ui.ftl` with placeholder keys for every string identified in **Current state** (can be stubs initially).

### Step 2 — Catalog: enumerate all keys

Walk every file in **Current state**, assign a key per unique user-visible string. Naming convention: `{panel}-{purpose}`, e.g. `bookmarks-panel-title`, `bookmarks-empty-label`, `history-search-placeholder`, `downloads-active-title`. Avoid abbreviations. Fill both `.ftl` files. Add plural variants for all parameterized strings.

### Step 3 — Externalize strings panel by panel

Replace inline literals panel by panel (start with the smallest panels first to validate the pattern):

1. `permission_panel.rs` (small, English-only)
2. `a11y_panel.rs`
3. `read_later_panel.rs`
4. `focus_panel.rs`
5. `sidebar_panel.rs`
6. `bookmark_panel.rs`
7. `history_panel.rs` (includes date formatting)
8. `command_palette.rs` (14 action labels)
9. `settings_panel.rs` (section tab labels)
10. `cert_panel.rs`
11. `privacy_panel.rs`
12. `shields_panel.rs`
13. `ai_panel.rs`
14. `note_viewer.rs`
15. `workspace_panel.rs`
16. `shortcuts_panel.rs`
17. `print_panel.rs`
18. `download.rs` (pluralized title)
19. `address_bar.rs` (omnibox badge labels and placeholder)
20. `devtools/inspector.rs`, `devtools/console_panel.rs`, `devtools/network_panel.rs`
21. `newtab.rs` (HTML template string in `build_newtab_html`)

Each panel: one commit, clippy clean, existing unit tests still pass (they assert on specific strings — update assertions to use the `en` catalog values, or expose the key name from the locale for test purposes).

### Step 4 — Language picker in Settings UI

Add a dropdown row in Appearance section of `settings_panel.rs`. Available choices: `"en"` (English), `"ru"` (Русский). Selecting a language writes `ui_language` via `BrowserSettings::set_ui_language`, reconstructs `Locale`, triggers a redraw. No restart required.

### Step 5 — OS locale detection

In `Locale::load("")`: on Windows check `LANG` env var, then call `GetUserDefaultLocaleName` via `winapi` (already a transitive dep) or parse `HKCU\Control Panel\International\LocaleName`. On Linux/macOS check `LANG` / `LC_ALL`. Map BCP 47 tag to the two-letter language code (`en`, `ru`). Fall back to `en` for any unrecognised tag.

---

## Dependencies

| Crate | Version | Category | Justification | Graduation criterion |
|---|---|---|---|---|
| `fluent` | `0.16` | Provisional | Mandated by `docs/plan/features.md` §10.8; handles Russian pluralization and gender that a plain key→string map cannot. Pure Rust, no C FFI. | Graduate to permanent when three or more locales ship. |

No other new dependencies. `icu_locale` / `icu_segmenter` are already in the workspace via `lumen-encoding`; if locale parsing needs BCP 47 normalization, use `icu_locale_core` (already in `Cargo.lock`) rather than adding a separate dep.

---

## Tests

1. **Unit — `locale.rs`:** `t("unknown-key")` returns a non-empty fallback (the key itself or `"??"`); `tn("downloads-active-title", 1)` returns the expected Russian singular; `tn("downloads-active-title", 5)` returns the expected Russian plural.
2. **Unit — `browser_settings.rs`:** `ui_language()` round-trips through `set_ui_language()`, default is `""`.
3. **Panel smoke tests (existing):** existing `draw_*` unit tests in each panel file assert specific strings. After externalization they must pass using the English locale (the test harness constructs `Locale::load("en")`). Where tests currently `assert!(text == "No bookmarks")`, keep that assertion — just ensure the `en` catalog maps `bookmarks-empty-label = No bookmarks`.
4. **Integration:** `cargo test -p lumen-shell` must pass with no regressions.

No graphic tests required for localization (text glyph diffs are excluded from the visual test suite per `CLAUDE.md`).

---

## Definition of done

- Every user-visible string in `lumen-shell` panels is looked up via `locale.t(key)` or `locale.tn(key, count)`.
- `assets/locales/en/ui.ftl` and `assets/locales/ru/ui.ftl` cover all keys; no key returns a fallback `??` value.
- Settings → Appearance contains a language selector; switching from `en` to `ru` (or back) changes all panel labels without restart.
- `cargo clippy -p lumen-shell --all-targets -- -D warnings` passes.
- `cargo test -p lumen-shell` passes.
- `crates/storage/src/browser_settings.rs` has `ui_language` key with getter/setter.
- `CAPABILITIES.md` updated (new capability: UI localization).
- `CSS-SPECS.md` — no changes needed (CSS task, not CSS property).
- `docs/plan/features.md` §10.8 implementation note updated.
