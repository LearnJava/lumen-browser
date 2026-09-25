# BUG-623: `window.find()` (legacy text-search API) is missing entirely

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `window` object)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`): `typeof window.find` → `"undefined"`.

`inert-and-find-flat-tree.html`'s two subtests both fail with `TypeError:
window.find is not a function` (`window.find('inside shadowroot')` /
`window.find('slotted')`, testing that `window.find` respects the flat
tree when searching text inside a `<dialog>` nested in a shadow root).
`inert-and-find.html` TIMEOUTs on the same call used at the top level of
its script (before any test registers).

## Масштаб

2 files in this category. `window.find` is a long-standing, non-standard
(not in any W3C/WHATWG spec) but widely-implemented legacy API — Firefox
and Safari both support it, Chrome does not by default. Lower priority
than the other findings in this category (it isn't a spec conformance gap
in the strict sense), but it is exercised directly by these two vendored
WPT tests and by any future category testing find-in-page-adjacent
behavior against shadow DOM. Not investigated whether Lumen's user-facing
find-in-page feature (`CAPABILITIES.md` lists find-in-page as ✅) could be
exposed here, or whether this needs its own independent text-search
implementation.

## Исправление (P3, 2026-09-25)

`window.find(string, caseSensitive, backwards, wrapAround)` добавлен в
`crates/js/src/shim/web_api_shim_tail_b.js` рядом с `window.focus`/`blur`.
Собственный текстовый поиск, не переиспользование find-in-page оболочки:
та работает по layout-боксам в процессе shell, а `window.find` синхронен и
должен видеть DOM скрипта (включая узлы, ещё не прошедшие layout).

- Обход — по плоскому дереву: у хоста вместо light-детей берутся дети shadow
  root, у `<slot>` — назначенные узлы хоста (фолбэк, если назначенных нет).
- Пропускаются: `inert`, `hidden`, `display:none` (computed), закрытый
  `<dialog>`, нерендеримые контейнеры (`head`/`script`/`style`/`template`/
  `iframe`/поля ввода), а при открытом модальном диалоге — весь текст вне
  верхнего из них (он делает остальной документ инертным).
- Текстовые узлы склеиваются в одну строку, так что совпадение может
  пересекать границы inline-элементов; найденное выделяется через
  `_lumen_set_selection`, следующий вызов стартует от конца выделения
  (при `backwards` — от начала). Без `wrapAround` вызов после последнего
  вхождения даёт `false` — ровно то, что проверяет цикл `testFindable`.

Регрессия — `crates/js/src/dom/tests/v8_bug623_window_find.rs` (5 тестов).

Живой WPT (`run_smoke.py`, dev-release):

| Файл | До | После |
|---|---|---|
| `inert/inert-and-find-flat-tree.html` | ERROR (`is not a function`) | OK 2/2, `.ini` удалён |
| `css/css-ui/interactivity-inert-find.html` | 0/3 | 2/3 |
| `inert/inert-and-find.html` | FAIL | FAIL (другая причина) |

Остатки, не относящиеся к `window.find`:

- `interactivity-inert-find.html` «Fail to find inert text» — CSS-свойство
  `interactivity: inert` не реализовано (домен P4), `.ini` сужен до этого
  подтеста.
- `inert-and-find.html` падает ДО вызова `find`:
  `iframe.contentWindow.getSelection(...).removeAllRanges is not a function` —
  фолбэк `wrapWinFacadeGlobals` (`frame_bridge.rs`) возвращает результат
  кросс-реалмного вызова JSON-копией без методов; класс BUG-1099.

`run_report.py --all --root inert --check` с фиксом и без него (та же
сборка, откат одного файла шима): список REGRESSION отличается ровно двумя
подтестами `inert-and-find-flat-tree.html`; остальные 29 записей — дрейф
`.ini` категории, общий для обеих сборок.
