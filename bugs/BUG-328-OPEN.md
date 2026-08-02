# BUG-328: `HTMLCollection` — `.item()` возвращает элемент с `id` вместо ожидаемого IDless, и `qux`/порядок в `ownKeys` расходятся со спеком

**Статус:** OPEN
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
