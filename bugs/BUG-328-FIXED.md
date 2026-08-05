# BUG-328: `HTMLCollection` — `.item()` возвращает элемент с `id` вместо ожидаемого IDless, и `qux`/порядок в `ownKeys` расходятся со спеком

**Статус:** FIXED 2026-08-05
**Дата:** 2026-07-22
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`, `_lumen_make_html_collection` /
`_lumen_html_collection_own_names`)
**Найден:** прогон `tests/wpt/run_suite.py` (curated gate) через новый лаунчер
`tests/wpt/run.ps1` — `dom/nodes/Element-children.html` снова красный (2 unexpected
против закоммиченного `metadata/dom/nodes/Element-children.html.ini`, который
[BUG-322](BUG-322-FIXED.md)/[BUG-323](BUG-323-FIXED.md) флипнули в `expected: PASS` 2026-07-21).

**Обновление 2026-08-02** (замечено попутно во время гейт-проверки WPT-RUN-2,
`tests/wpt/run_suite.py`): сабтест 1 (`item("foo")` → IDless) теперь **проходит** —
что-то в промежутке починило коэрсию/порядок, независимо от этого бага. Сабтест 2
(`ownKeys`/`qux`/порядок) по-прежнему красный, симптом идентичен описанному ниже.
`.ini` перепиннут по рекомендации из раздела «Что нужно» п.2: сабтест 1 →
`expected: PASS` (уже было верно), сабтест 2 → `expected: FAIL` со ссылкой на этот
баг.

## Симптом

Оба сабтеста `Element-children.html` («HTMLCollection edge cases» / «… 1») снова FAIL:

```
FAIL HTMLCollection edge cases
  assert_false: Expected the IDless Element. expected false got true

FAIL HTMLCollection edge cases 1
  assert_array_equals: lengths differ,
    expected ["0","1","2","3","4","5","foo","bar","baz"] length 9,
    got      ["0","1","2","3","4","5","foo","baz","bar","qux"] length 10
```

Тестовое дерево (`tests/wpt/dom/nodes/Element-children.html`): `#test` содержит
`<img> <img id=foo> <img id=foo> <img name=bar>` в разметке, плюс два элемента,
добавленных в `setup()` через `createElementNS("", "img")` — один с `id=baz`, один
с `name=qux`.

1. **Сабтест 1** — `container.children.item("foo")` должен по WebIDL сначала привести
   `"foo"` через `ToUint32` (не найти по имени!) → `NaN → 0` → вернуть `list[0]`, первый
   `<img>` без атрибутов. Ожидание: `result.hasAttribute("id") === false`. Наблюдаем `true`
   — т.е. либо коэрсия индекса не срабатывает для этого случая, либо `list[0]` — не тот
   элемент, что предполагает тест (возможен сдвиг порядка после `appendChild` в `setup()`).
   Код `.item` (`dom.rs:5434-5439`) на вид корректно делает `i = i >>> 0` — фактическое
   поведение живого движка не воспроизведено интерактивно в рамках этой находки, нужен
   отдельный BiDi/REPL-прогон для локализации (сравнить `ids()` на живом дереве до/после
   `setup()` с ожидаемым порядком).
2. **Сабтест 2** — `Object.getOwnPropertyNames(container.children)` должен быть
   `['0','1','2','3','4','5','foo','bar','baz']` (9 элементов, `qux` не экспонируется).
   Наблюдаем 10 элементов, включая `qux`, и другой порядок (`baz` перед `bar`). Это
   **тот самый гэп, что BUG-323's фикс уже документировал как осознанно не тронутый**
   (`bugs/BUG-323-FIXED.md`, раздел «Не тронуто»): Lumen сворачивает
   `createElementNS("", "img")` в `Namespace::Html` (`_lumen_create_element_ns`), поэтому
   `_lumen_html_collection_own_names` (`dom.rs:5409-5421`) не может отличить настоящий
   HTML-элемент от «no-namespace»-элемента и экспонирует оба по `name`. DOM Standard
   §4.2.10.2 требует, чтобы `name`-экспозиция ограничивалась элементами именно в HTML
   namespace.

## Почему это `unexpected`, а не давно известный `expected: FAIL`

`.ini` был флипнут в `expected: PASS` для обоих сабтестов 2026-07-21 при закрытии
BUG-322 (`instanceof`), но комментарий самого BUG-323 уже предупреждал, что второй
сабтест (`qux`/порядок) не закроется одним только фиксом enumeration-traps или
instanceof — нужна ещё и namespace-проверка в `_lumen_html_collection_own_names`
(и, зеркально, в `_lumen_html_collection_named`, если она тоже должна различать
namespace). Похоже, флип в `.ini` 2026-07-21 не учёл эту оговорку.

## Что нужно

1. Разобраться с сабтестом 1 (`item("foo")` → неожиданный `id`) — отдельная, возможно
   независимая от namespace-гэпа причина; нужна пошаговая проверка живого дерева.
2. Добавить различение namespace (HTML vs no-namespace) в `_lumen_html_collection_own_names`
   и `_lumen_html_collection_named`, либо задокументировать, что `createElementNS("", ...)`
   сворачивание в HTML namespace — постоянный, некосметический предел, и снова закрепить
   `expected: FAIL` в `.ini` с явной ссылкой на этот баг (а не молча откатывать в `FAIL`
   без объяснения, как предостерегает сам `.ini`-файл).

## Как воспроизвести

```powershell
$env:LUMEN_PROFILE = "dev-release"
$BIN = "$PWD\target\dev-release\lumen.exe"
tests\wpt\.venv\Scripts\python.exe tests\wpt\run_smoke.py --binary "$BIN" /dom/nodes/Element-children.html
```

или через новый единый лаунчер: `tests\wpt\run.ps1` (curated gate, без `--metadata`
override увидит расхождение как unexpected) / `tests\wpt\run.ps1 -Report` для HTML-отчёта.

## Резолюция (2026-08-05, P3)

Сабтест 2 (`ownKeys`/`qux`/порядок) падал на ДВУХ независимых дефектах, а не
одном:

1. **Namespace-гэп (описан выше).** `Namespace` (`crates/engine/dom/src/lib.rs`)
   не имел варианта «нет namespace» — `_lumen_create_element_ns` (`v8_runtime.rs`)
   сворачивал любой не-SVG namespace (включая пустую строку) в `Namespace::Html`.
   Добавлен `Namespace::None` (namespaceURI = `null`, DOM §4.5 «validate and
   extract»); `_lumen_create_element_ns` теперь мапит `ns.is_empty()` →
   `Namespace::None` вместо HTML-фолбэка. Заодно исправлена сама главная
   `document.createElementNS` (`dom.rs`, объект `document` из ~4176 строки) —
   она передавала `String(ns)` без нормализации `null`/`undefined` в `''`, из-за
   чего `createElementNS(null, ...)` слал буквальную 4-символьную строку
   `"null"` (уже не пустую, не SVG-URL → HTML-фолбэк) — тот же класс дефекта,
   что и основной, только на другом входе. `_lumen_build_detached_document`'s
   вариант (`createDocument`-детач-документы) такую нормализацию уже делал
   правильно.
2. **Структура прохода в `_lumen_html_collection_own_names`/
   `_lumen_html_collection_named` (`dom.rs`).** Обе функции делали ДВА
   отдельных прохода по всей коллекции — сперва все `id`, потом все `name` —
   вместо одного прохода в document order, где для каждого элемента сначала
   проверяется `id`, затем (только для HTML-namespace) `name`. DOM §4.2.10.2
   определяет именно единый проход per-element; двухпроходная структура
   давала верный НАБОР имён, но неверный ПОРЯДОК всякий раз, когда элемент с
   `name` предшествует в дереве элементу с `id` (тестовое дерево: `name=bar`
   на индексе 3, `id=baz` на индексе 4 — спек требует `['bar','baz']`, старый
   код давал `['baz','bar']`, потому что все `id` собирались первым проходом
   независимо от позиции в дереве). Даже после фикса namespace-гэпа (п.1)
   сабтест 2 продолжал падать именно на этой перестановке, что и вскрыло
   второй дефект. Обе функции переписаны на единый tree-order проход;
   `_lumen_html_collection_named` заодно избавлен от того же структурного
   бага (для одного `name`-параметра он был семантически безвреден на
   данных этого теста, но некорректен в общем случае: элемент с более
   ранним name-совпадением мог проигрывать более позднему по дереву
   id-совпадению).

Регресс-тест `bug_328_html_collection_name_exposure_requires_html_namespace`
(`crates/js/src/dom.rs`, `dom::tests::v8_childnode_traversal`) воспроизводит
ровно дерево `Element-children.html` (`<img> <img id=foo> <img id=foo>
<img name=bar>` + два `createElementNS("", "img")`) и проверяет
`namespaceURI === null`, точный порядок `ownKeys`, и что `namedItem` находит
no-namespace элемент по `id`, но не по `name`.

`dom/nodes/Element-children.html.ini`: оба сабтеста → `expected: PASS`. Живой
прогон подтверждён: `run_smoke.py` (одиночный тест) 2/2 subtests OK, весь
curated gate `run_suite.py` — 61/61 checks OK, 0 unexpected (без регрессий в
остальных 19 `dom/nodes` тестах). `graphic_tests/dump_golden.py` — 12/12 без
изменений (чисто JS/DOM-семантика, дисплей-лист не затронут).
