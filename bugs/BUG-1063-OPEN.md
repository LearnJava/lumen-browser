# BUG-1063 — CSS-селекторы с namespace-префиксом `*|` / `|` (`*|body`, `*|*`, `|body`) отвергаются как невалидные, из-за чего умирает любой `test_driver.click`/`send_keys` на элементе без `id`

**Статус:** OPEN
**Тип:** дефект реализованного кода — `querySelector`/`querySelectorAll`/`matches`/`closest` бросают `SyntaxError` на синтаксически корректном селекторе с типом-в-namespace (CSS Namespaces Level 3 §6, `*|E`, `|E`, `*|*`).
**Заведён:** 2026-09-19 (WPT-RUN-7 срез 36, категория `shadow-dom`)
**Область:** `crates/engine/css-parser/src/parser/selectors.rs` (`parse_compound_selector`, `:1138`) → `layout/src/selector_query.rs`; ошибку бросает шим `_lumen_sel` (`crates/js/src/shim/web_api_shim_head.js:104`) по вердикту `_lumen_selector_is_valid`.
**Владелец:** P3 (P2 багов не чинит).

## Симптом

Проба вне WPT (`--dump-layout` страницы, где каждый селектор вызывается через
`document.querySelector`; страница — `<html><head/><body>…`):

| селектор | результат | ожидание |
|---|---|---|
| `:root > body` | `BODY` | `BODY` |
| `:root > body:nth-child(2)` | `BODY` | `BODY` |
| `*\|body` | `SyntaxError` | `BODY` |
| `:root > *\|body` | `SyntaxError` | `BODY` |
| `:root > *\|body:nth-child(2)` | `SyntaxError` | `BODY` |
| `\|body` | `SyntaxError` | `null` (пустой namespace — у HTML-элементов его нет) |
| `*\|*` | `SyntaxError` | первый элемент |
| `*\|p#out` | `SyntaxError` | `P` |
| `svg\|rect` (без `@namespace`) | `SyntaxError` | `SyntaxError` — **верно**, префикс не объявлен |

Все три пары «с префиксом»/«без» различаются ровно префиксом `*|`: `:nth-child`, `:root`
и комбинатор `>` движок разбирает нормально.

## Почему это заметно в WPT — цепочка

`tools/wptrunner/wptrunner/testdriver-extra.js::get_selector` (вендоренный, стандартный
для любого продукта) сериализует элемент **без `id`** путём

```js
let segment = "*|" + el.localName;            // testdriver-extra.js:139
segments.push(segment + ":nth-child(" + nth + ")");
```

то есть `document.body` превращается в `:root > *|body:nth-child(2)`. Исполнитель
(`executorlumen.py::_resolve_element_center`, `_action_send_keys`, `_action_click`) отдаёт
этот селектор обратно странице через `querySelector` — и движок кидает `SyntaxError`, файл
завершается `ERROR`. Идиома `test_driver.send_keys(document.body, "\t")` — основа тестов
фокус-навигации, так что удар приходится ровно по ним.

**Срез 36, `shadow-dom`:** 51 из 63 файлов со статусом `TEST_END: ERROR` — именно это
сообщение (`eval: JS runtime error: :root > *|body:nth-child(2) is not a valid selector`);
`.ini` с `ERROR` лежат в основном в `shadow-dom/focus-navigation/` (37 файлов) и
`shadow-dom/focus/` (18) — эта цифра считает и подтесты, так что как точный счёт «51 по
папкам» её брать нельзя. Файл падает целиком, поэтому его сабтесты не измерены вовсе —
это не «сабтест упал», а «предмет теста не проверен».

Класс шире категории: сериализатор одинаков для любого продукта и любой категории. В
вендоренном корпусе `test_driver.click`/`send_keys` вызывают 161 файл (297 вызовов;
`rg 'test_driver\.(send_keys|click)\('` по `tests/wpt`) — `clipboard-apis`, `close-watcher`,
`css/selectors` (`focus-visible-*`, `user-invalid`), `editing`, `fullscreen`, `focus`,
`event-timing`. Сколько из них передаёт элемент **без `id`** (только такие идут через
`*|`-путь; элемент с `id` идёт по `#\61 \62 …` и упирается в другой дефект —
[BUG-1065](BUG-1065-OPEN.md), эскейпы в селекторах), срез не считал.

## Что НЕ проверено

- Разбор `*|E` в **таблицах стилей** (`@namespace` + `ns|E { … }`, `*|E { … }`): проба
  касалась только API `querySelector*`, каскад не мерили. `CSS-SPECS.md:619` фиксирует
  `@namespace` как «parsed; no XML namespaces», так что не исключено, что правило
  каскада молча теряется по той же причине.
- Совпадение `*|E` с элементами не из HTML-namespace (SVG/MathML): нужен `Namespace::Other`
  из GAP-XMLDOC, порядок работ с ним — на усмотрение исполнителя.

## Ожидание / приёмка

`*|E` и `*|*` совпадают с элементом независимо от его namespace; `|E` — только с
элементами без namespace (для HTML-документа — ни с одним); `ns|E` без объявления
префикса по-прежнему `SyntaxError` (проба выше, последняя строка таблицы). Регрессионный
тест — строка из проб выше; косвенная приёмка — после починки `shadow-dom/focus-navigation`
перестаёт давать `ERROR` из-за `is not a valid selector`, а baseline среза 36 придётся
перегенерировать (`--update-expected` + три `--check`).

## Побочное следствие для baseline

Baseline `tests/wpt/metadata/shadow-dom/**` записан с `expected: ERROR` для этих файлов.
После фикса они перестанут быть `ERROR` → `--check` покажет их как unexpected-pass /
deviation; это ожидаемо, а не регрессия — baseline регенерируется тем же коммитом, что
и фикс (или сразу следом).

## Срез 44 WPT-RUN-7 (2026-09-21, `pointerevents`)

`pointerevents` (258 id): **12 id** упали на `*|`-пути этого бага (`:root > *|body:nth-child(2)`), ещё **152** — на ветке с `id` ([BUG-1065](BUG-1065-OPEN.md)); вместе 164 из 258 (64 %) — harness-`ERROR` до первого подтеста. Baseline записан с `expected: ERROR` для этих файлов — нижняя планка, гейт по ним пуст, пока баг не закрыт; после фикса регенерировать (`--update-expected` + три `--check`), сдвиг `ERROR → OK/FAIL/TIMEOUT` ожидаем, а не регрессия.

## Срез 47 WPT-RUN-7 (2026-09-21, `editing`)

`editing` (700 id после раскрытия `?…`-вариантов): **263 из 293 harness-`ERROR`** — `eval: JS runtime error: :root > *|body:nth-child(N) > *|div:nth-child(M) … is not a valid selector`
(суффиксы `*|ul`, `*|ol`, `*|dl`, `*|span`, `*|img` — те же). Это самая массовая причина `ERROR` из всех снятых категорий (`pointerevents` — 12 id, `shadow-dom` — единицы):
`editing/other/*.html`, `editing/run/*.html` и `editing/plaintext-only/*.html` открываются через `test_driver.click`/`send_keys` на элементе без `id`, а
`testdriver-extra.js::get_selector` строит именно такой путь. Для сравнения: 382/700 id проходят harness, то есть после починки этого бага и [BUG-1065](BUG-1065-OPEN.md)
(ещё 13 id) остаётся не больше ~17 `ERROR`, а ~276 файлов перейдут с `ERROR` в реальные подтесты — категория `editing` станет заметно полезнее как гейт.
Baseline `tests/wpt/metadata/editing/**` записан с `expected: ERROR` для этих файлов — нижняя планка; регенерировать после починки (`--update-expected` + три `--check`).
Пример одиночного файла: `/editing/other/cloning-attributes-at-splitting-element.tentative.html`.
