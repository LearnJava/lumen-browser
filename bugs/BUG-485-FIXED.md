# BUG-485: `document.head` is entirely missing — no getter, no native binding

**Статус:** FIXED 2026-08-09
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — live `document` object literal, `~7159-7184`)
**Найден:** WPT-RUN-3 срез 6 (`ROADMAP.md`) — массовый прогон `css/css-cascade`

## Механизм

The live `document` singleton object (`dom.rs:~7159`) defines `get body()`
(backed by the native `_lumen_get_body` binding, registered on the V8 path,
`v8_runtime.rs:1205`), but there is no matching `get head()` anywhere, and no
`_lumen_get_head`/equivalent native function exists in `dom.rs`/`v8_runtime.rs`.
So `document.head` resolves via normal property lookup to plain `undefined` —
not a thrown error, not `null` — because the property was never declared at
all.

Confirmed by source inspection (grepped, zero hits for
`_lumen_get_head`/`get head`) and cross-checked live via `--mcp-live-port`
(`typeof document.head` → `"undefined"`).

*Updated P1, 2026-08-04 (P3-v8-post-audit): at filing time (2026-08-02) `_lumen_get_body`
was still mirrored on the rquickjs path too; that path was removed entirely in
S12b-F3, so V8 is now the only engine in scope for the fix below.*

## Симптом

`document.head` is the standard, spec-mandated (HTML LS §3.1.2) place WPT
tests inject a `<style>` element to change the page's stylesheet dynamically
— the single most common idiom in cascade/CSSOM tests:

```js
const styleElement = document.createElement('style');
styleElement.textContent = testCase.style;
document.head.appendChild(styleElement);   // or .append(styleElement)
```

`.appendChild`/`.append` on `undefined` throws a synchronous `TypeError:
Cannot read properties of undefined (reading 'appendChild'|'append')`. In
every file in this slice the injection loop sits at the **top level of the
`<script>`**, outside any `test()`/`promise_test()` callback (tests are
registered dynamically, one per stylesheet variant) — so the throw happens
before a single test is registered with `testharness.js`. The harness then
never calls `done()` and the run sits until the external timeout: this is
why the symptom is **TIMEOUT**, not a clean `FAIL`, for every file that hits
this pattern before its first `test()` call, and a `FAIL`/rejected-promise
for files that only reach it inside a `promise_test()` body.

## Масштаб находки

**15 files in this slice (`css/css-cascade`)** — confirmed by grepping each
failing file's source for `document.head.` and matching against its actual
failure text (`Cannot read properties of undefined (reading 'appendChild'|
'append')`, or `TIMEOUT` with the injection call sitting before the first
`test()`):

- **TIMEOUT** (throw happens before any test registers): `layer-basic.html`,
  `layer-important.html`, `layer-vs-inline-style.html`,
  `layer-keyframes-override.html`, `layer-counter-style-override.html`,
  `layer-property-override.html`, `layer-import.html` — 7 files.
- **FAIL / unhandled promise rejection** (throw happens inside a
  `promise_test()`, so the harness survives and reports it): `import-
  conditions.html` (26 subtests), `parsing/supports-import-parsing.html` (22),
  `parsing/layer.html` (11), `parsing/layer-import-parsing.html` (13),
  `layer-cssom-order-reverse.html` (4), `layer-cssom-order-reverse-at-
  property.html` (2), `layer-font-face-override.html` (4), `layer-
  statement-before-import.html` (7) — 8 files, 89 subtests.

Not css-cascade-specific — `grep -rl 'document\.head\.' tests/wpt/css --
include=*.html \| wc -l` → **84 files** in the vendored `css/` tree alone use
this idiom, so it will recur in every future WPT-RUN-3 slice that dynamically
injects a stylesheet. Second-highest-leverage fix in this slice after
verifying scope (behind [BUG-471](BUG-471-OPEN.md)'s CSSOM stylesheet model,
which is a larger undertaking; this one is a single accessor).

## Что нужно

Add `get head()` to the `document` object literal (`dom.rs:~7163`, right next
to `get body()`), backed by a new native `_lumen_get_head` binding in
`v8_runtime.rs` mirroring `_lumen_get_body`'s pattern (walk `documentElement`'s
children for the first `<head>`, matching `_lumen_get_body`'s equivalent
walk for `<body>`). Per HTML LS §3.1.2, `document.head` must also be
settable-adjacent in that a document is only ever built with the standard
`html > head, body` skeleton by the engine's own DOM construction, so a pure
getter (no setter — spec doesn't define one for `head`, only `body` has a
setter and even that isn't implemented here) is sufficient to match spec
shape.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-cascade/` for the 15
attributed files, `expected: FAIL`/`TIMEOUT` matching the actual run.

## Срез 16 (`css/css-forms`, 2026-08-02) — same idiom, а new call site (`test_valid_selector`)

`tests/wpt/css/support/parsing-testcommon.js::test_valid_selector` (shared
helper, not specific to css-forms) does `document.head.append(style)` as its
first step after the `document.querySelector(selector)` no-throw check —
every "should be a valid selector" subtest in any file that uses this helper
therefore throws on `document.head` being `undefined` before it ever reaches
the actual selector-validity assertions this bug's fix would unblock. 3 files
this slice: `parsing/checkmark-pseudo-element.html` (5 subtests),
`parsing/picker-icon-pseudo-element.html` (5), `parsing/picker-select-
pseudo-element.html` (11) — 21 subtests total. Confirms the 84-file `css/`
grep estimate from срез 6 undercounts call sites that go through this shared
test helper rather than inline `document.head.` usage — worth re-grepping
`document\.head\.` **and** call sites of `test_valid_selector`/`test_valid_
rule`/`test_invalid_rule` (`parsing-testcommon.js`, all four use the same
`document.head.append(style)` pattern) before estimating blast radius again.
`.ini` under `tests/wpt/metadata/css/css-forms/` for all 3 files, `expected:
FAIL` per affected subtest.

## Срез 26 (`css/css-counter-styles` + `css/css-highlight-api` + `css/css-page`, 2026-08-03) — two more call sites

`support/counter-style-testcommon.js` (shared helper, all 10
`counter-style-at-rule/*.html` files this slice) — same
`document.head.appendChild(style)` idiom, at top level before the first
`test()` in every case, so all 10 are TIMEOUT rather than FAIL.
`highlight-pseudo-parsing.html`'s "should be a valid selector" subtests (6)
go through `parsing-testcommon.js`'s `test_valid_selector` (the срез-16 call
site) again. `css-page/parsing/margin-rules-001.html` (15) and
`parsing/page-rules-001.html` (8) go through the sibling
`test_valid_rule`/`test_invalid_rule` helpers in the same file. `.ini` under
`tests/wpt/metadata/css/css-counter-styles/`,
`tests/wpt/metadata/css/css-highlight-api/`, and
`tests/wpt/metadata/css/css-page/`.

## Срез 29 (`css/css-shadow` + `css/css-scroll-snap` + `css/css-animations`, 2026-08-03) — largest single-slice count, ~182 subtests

Re-confirmed live via a minimal `--dump-layout` probe (`document.head` →
`undefined` while `document.body`/`document.documentElement`/
`document.querySelector('head')` all resolve correctly — rules out a general
element-lookup regression, isolates the accessor itself). Two call-site
shapes: `test_valid_selector`/`test_invalid_selector` in
`parsing-testcommon.js` (`.append`, srez-16/26 call site, dominant here too
— 172 subtests, mostly `css-shadow`'s `host-context-parsing.html` and
similar) and a direct `document.head.appendChild(originalStyleElement)` in
`css-animations/animation-style-element-replaced-with-keyframes-rule-of-
same-name.html` (`.appendChild`, 10 subtests, a new call-site shape not
previously catalogued in this bug — worth re-grepping
`document\.head\.appendChild` separately from `\.append\(` before the next
blast-radius estimate).

Systemic note for whoever fixes this: because
`test_valid_selector`/`test_invalid_selector` is the single most common
building block for CSS selector-parsing WPT tests across the *entire*
vendored `css/` corpus, this bug likely also explains a share of
"selector should be valid/invalid" failures already attributed to other
causes in earlier slices (1-28) that used the same helper without isolating
`document.head` specifically — re-verify old findings after this lands,
same caution as [BUG-539](BUG-539-OPEN.md)'s note about its own systemic
reach. `.ini` under `tests/wpt/metadata/css/css-shadow/`,
`tests/wpt/metadata/css/css-scroll-snap/`,
`tests/wpt/metadata/css/css-animations/`.

## Исправлено (P3, 2026-08-09, в ходе разбора [BUG-703](BUG-703-FIXED.md))

Добавлены нативный `_lumen_get_head` (сосед `_lumen_get_body`, тот же обход
дерева — первый `<head>` в порядке документа) и геттер `document.head` в
живом объекте `document` (`crates/js/src/dom.rs`); заодно `head`/`body`
появились у отсоединённых документов (частично закрывает
[BUG-415](BUG-415-FIXED.md)).

Найдено заново на живой странице `https://www.tbank.ru/`: загрузчик чанков
webpack заканчивается на `document.head.appendChild(script)`, поэтому на
любом бандл-сайте каждый ленивый чанк падал с `TypeError: Cannot read
properties of undefined (reading 'appendChild')` — внутри асинхронного
бутстрапа без следа.

Тесты: `document_head_is_the_head_element`,
`document_head_accepts_appended_script`,
`detached_document_exposes_head_and_body` в `dom::tests::v8_core`.
Сабтесты WPT заново не измерялись — прогонов категорий в этой сессии не было.

Дубликат: [BUG-485](BUG-485-FIXED.md) и [BUG-565](BUG-565-FIXED.md) — один и
тот же дефект, заведённый дважды разными срезами WPT (WPT-RUN-3 срез 6 и
WPT-VENDOR-html-semantics-document-metadata); закрыты одним фиксом.
