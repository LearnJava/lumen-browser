# BUG-486: `document.currentScript` is entirely missing

**Статус:** FIXED 2026-08-09 (P3, при разборе [BUG-703](BUG-703-FIXED.md))
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — live `document` object literal),
shell (`crates/shell/src/main.rs` — парсерный путь исполнения скриптов)
**Найден:** WPT-RUN-3 срез 6 (`ROADMAP.md`) — массовый прогон `css/css-cascade`

## Механизм

`document.currentScript` (HTML LS §8.1.3.4) — the `<script>` element
currently being executed, or `null` outside a script's synchronous
top-level execution — is not defined anywhere in the JS shim. Grepped both
`dom.rs` and `v8_runtime.rs` for `currentScript`: zero hits. Bare property
access on the live `document` object therefore falls through to plain
`undefined` rather than the spec-mandated `HTMLScriptElement` reference (or
`null`).

Confirmed live via `--mcp-live-port`: `typeof document.currentScript` →
`"undefined"`.

## Симптом

Three files in this slice (`css/css-cascade`) — `scope-evaluation.html`,
`scope-invalidation.html`, `scope-proximity.html` — share one helper defined
inline at the top of each:

```js
function test_scope(script_element, callback_fn, description) {
  test((t) => {
    let template_element = script_element.previousElementSibling;
    // ...
```

called throughout each file as `test_scope(document.currentScript, () => {
...`. With `document.currentScript` resolving to `undefined`, every call
passes `undefined` as `script_element`, and `.previousElementSibling` on
`undefined` throws `TypeError: Cannot read properties of undefined (reading
'previousElementSibling')` — inside the `test()` callback, so each
individual test reports this as its `FAIL` message rather than crashing the
whole file (harness status stays `OK`, only the subtests fail).

## Масштаб находки

**3 files / 55 subtests** in this slice, 100% attributable — every failing
subtest in these three files carries the identical
`previousElementSibling`-on-`undefined` message, confirmed by reading each
file's full subtest list (no other defect masked underneath):
`scope-evaluation.html` (22), `scope-invalidation.html` (28),
`scope-proximity.html` (5).

Note: all three of these files' `<main id=main>` markup also relies on named
access on Window for other assertions elsewhere in the same pattern family
(the [BUG-384](BUG-384-OPEN.md) mechanism) — but `document.currentScript`
fails **first**, before named access on `main` is ever exercised, so
`document.currentScript` is the correct primary/blocking attribution for
these three files. Fixing this bug alone will not turn every subtest green —
BUG-384 sits immediately behind it for the same files.

Not css-cascade-specific: `document.currentScript` is a generic HTML API
used by any test that needs to locate its own `<script>` tag relative to
sibling markup (a common testing idiom across WPT, not specific to CSS) —
expect this to recur in unrelated future categories.

## Что нужно

Add `get currentScript()` to the `document` object literal, backed by
runtime state that tracks the currently-executing `<script>` element's node
id during synchronous script evaluation (set on entry to each `<script>`'s
execution, cleared — per spec, to `null` — once that script's synchronous
run completes; `null` for the async/deferred/module cases per HTML LS
§8.1.3.4 step list). Both engines (`dom.rs` rquickjs path,
`v8_runtime.rs` V8 path) need the tracking hook wherever they currently
invoke a `<script>`'s source text.

## Фикс (2026-08-09)

Стек, а не одиночный слот: классический скрипт может синхронно вставить и
выполнить другой, и внешний обязан снова увидеть себя, когда вложенный вернёт
управление.

- `crates/js/src/dom.rs` — `_lumen_current_script_stack` +
  `_lumen_push_current_script`/`_lumen_pop_current_script`; пуш/поп обрамляют
  тело в `_lumen_script_execute_classic(text, nid)` (динамический путь: и
  inline, и внешний `<script src>`), геттер `document.currentScript` отдаёт
  вершину стека либо `null`. У detached-документа
  (`_lumen_build_detached_document`) свойство есть и всегда `null` — иначе
  фича-детект читает `undefined`.
- `crates/shell/src/main.rs` — парсерный путь: `ScriptSource::Inline`/`External`
  теперь несут `NodeId` своего `<script>`, `resolve_script_sources` протаскивает
  его до `run_scripts_with_dom`, который обрамляет каждый классический `eval`
  парой push/pop (включая ветки ошибок — иначе один упавший скрипт оставил бы
  протухшее значение всем следующим).
- Модули, обработчики событий и любые асинхронные колбэки читают `null`: стек к
  моменту задачи/микрозадачи уже пуст — ровно то, что требует спека.

Тесты (`cargo test -p lumen-js --features v8-backend current_script`, 4/4):
элемент виден изнутри себя вместе с `dataset`; `null` снаружи и свойство
существует; вложенный скрипт восстанавливает внешний; бросивший скрипт не
оставляет протухшего значения. Плюс
`resolve_script_sources_passes_inline_through` в shell проверяет, что каждое
тело несёт id своего элемента.

Проверка на живой странице (`tbank.ru`): микроблоки перестали регистрироваться
под ключом `undefined`, DOM 299 → 2382 элемента — подробности в
[BUG-703](BUG-703-FIXED.md).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-cascade/` for the 3
attributed files, `expected: FAIL` per the actual run. **Не пересматривались
этим фиксом намеренно:** за `currentScript` в тех же трёх файлах немедленно
встаёт [BUG-384](BUG-384-OPEN.md) (named access on Window), так что зелёными
они не станут; актуализировать метаданные должен прогон категории (P2), а не
догадка.
