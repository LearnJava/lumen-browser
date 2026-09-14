# BUG-572: `HTMLScriptElement.supports()` static method missing

**Статус:** FIXED 2026-09-14 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — HTML
tag-interface generation loop)
**Найден:** P2, WPT-VENDOR-html-semantics-scripting-1, 2026-08-04

## Симптом

```
FAIL HTMLScriptElement.supports resurns true for 'classic' - HTMLScriptElement.supports is not a function
FAIL HTMLScriptElement.supports resurns true for 'module' - HTMLScriptElement.supports is not a function
```

(typo "resurns" is upstream WPT's, not a transcription error).
`HTMLScriptElement.supports` is entirely absent — `typeof
HTMLScriptElement.supports === "undefined"`.

## Причина

Never implemented. `grep -n "HTMLScriptElement" crates/js/src/dom.rs` shows
only prototype reflection (`type`/`src`/`async`/`defer`/`noModule`/etc. via
`_lumen_install_reflection`); no static method table. Per HTML LS
§4.12.1.2, `HTMLScriptElement.supports(type)` is a static feature-detection
method (`"classic"`, `"module"`, `"importmap"` all `true`; anything else
`false`) that pages use to detect whether the engine understands a given
`<script type>` before relying on it (increasingly common for
`importmap`/module feature-detection).

## Масштаб

4 subtests, single file
(`html/semantics/scripting-1/the-script-element/script-supports.html`). Low
count, but `HTMLScriptElement.supports` is a documented, spec-stable static
and cheap to add given the classification logic already exists as
`is_classic_script_type()` (`crates/shell/src/scripts.rs`) — the JS shim
just needs a static function mirroring that whitelist plus the `module`/
`importmap` literals.

## Исправлено

Static `HTMLScriptElement.supports(type)` defined right after the HTML
tag-interface generation loop in `web_api_shim_mid.js` (next to the BUG-567
`HTMLTitleElement.text` fix, same file/area). Exact string comparison
against `classic`/`module`/`importmap` only (no trim, no case-folding — the
WPT test explicitly checks `' classic '`/`'Classic'`/`'classics'` all come
back `false`); every other input is `false`. All three recognized values
answer `true` — classic scripts, modules and import maps are all already
executed by the engine (`crates/shell/src/scripts.rs`).

Verified against the vendored WPT tests (dev-release build, real
`lumen.exe` over WebDriver BiDi via `tests/wpt/run_smoke.py`):
`html/semantics/scripting-1/the-script-element/script-supports.html` 5/5
and `import-maps/script-supports-importmap.html` 2/2.
