# BUG-1065 — CSS-эскейпы в селекторах (`#\61 bc`, `#a\62 c`, `#a\bc`) отвергаются как невалидные, из-за чего умирает `test_driver.click`/`send_keys` даже на элементе с `id`

**Статус:** OPEN
**Тип:** дефект реализованного кода — `querySelector`/`querySelectorAll`/`matches`/`closest` бросают `SyntaxError` на корректном селекторе с экранированным идентификатором (CSS Syntax L3 §4.3.7 «consume an escaped code point», CSSOM §«escape»).
**Заведён:** 2026-09-19 (WPT-RUN-7 срез 36, `shadow-dom`; найден при проверке утверждения для [BUG-1063](BUG-1063-OPEN.md))
**Область:** `lumen_css_parser::is_valid_selector_list` — её вердикт использует `_lumen_selector_is_valid` (`crates/js/src/v8_runtime/install/dom_core.rs:317`), после чего шим `_lumen_sel` (`crates/js/src/shim/web_api_shim_head.js:104`) бросает `SyntaxError`. Где именно в разборе теряется `\` — токенизатор или разбор compound-селектора — проба не различает.
**Владелец:** P3 (P2 багов не чинит).

## Симптом

Проба вне WPT (`--dump-layout`; в DOM есть `<div id="abc">` и `<div id="1x">`; обратная косая
собирается через `String.fromCharCode(92)`, чтобы не зависеть от экранирования в самой странице):

| селектор (`\` показан как `/`) | результат | ожидание |
|---|---|---|
| `#abc` | `abc` | `abc` |
| `#/61 bc` | `SyntaxError` | `abc` (`\61 ` = `a`) |
| `#a/62 c` | `SyntaxError` | `abc` |
| `#a/62c` | `SyntaxError` | `null` (`\62c` — шесть hex-цифр не набралось, кодпойнт U+062C) |
| `#/61 /62 /63 ` | `SyntaxError` | `abc` |
| `#/31 x` | `SyntaxError` | `1x` |
| `#/61bc` | `SyntaxError` | `null` (U+61BC) |
| `#a/bc` | `SyntaxError` | `abc` (identity-эскейп `\b`) |

Отвергаются все семь проверенных форм с обратным слешем в идентификаторе, включая простейший
identity-эскейп; без эскейпа тот же `id` находится.

## Почему это заметно в WPT

`tools/wptrunner/wptrunner/testdriver-extra.js::get_selector` для элемента **с `id`**
сериализует его так:

```js
selector = "#";
// escape everything, because it's easy to implement
for (...) selector += '\\' + id.charCodeAt(i).toString(16) + ' ';
```

то есть `id="abc"` → `#\61 \62 \63 ` — ровно строка из четвёртой пробы. Исполнитель
(`executorlumen.py::_resolve_element_center`, `_action_click`, `_action_send_keys`)
отдаёт её обратно странице, и `querySelector` бросает `SyntaxError`. Вместе с
[BUG-1063](BUG-1063-OPEN.md) (безымянные элементы, `*|`-путь) это закрывает **обе** ветки
`get_selector`: ни элемент с `id`, ни без него `test_driver.click`/`send_keys`/`action_sequence`
на элементе не работают. В вендоренном корпусе такие вызовы делают 161 файл (297 вызовов,
`rg 'test_driver\.(send_keys|click)\('` по `tests/wpt`); сколько из них передают элемент с `id`,
а сколько без, не считалось.

Срез 36 наткнулся только на `*|`-ветку (51 файл `shadow-dom` передают `document.body`) —
ветку с `id` он не видел, потому что файлы падали раньше.

## Что НЕ проверено

- Эскейпы в **классах, атрибутах, псевдоклассах** (`.a\:b`, `[data-x=\"y\"]`, `:not(#a\62 c)`).
- Эскейпы в **таблицах стилей** (каскад): проба касалась только API `querySelector*`. Если
  таблицы стилей рвутся так же, страница с `.sm\:flex { … }` (типичный Tailwind) теряет
  правила молча.
- `CSS.escape()` — как он ведёт себя и совпадает ли его вывод с тем, что разбирает парсер.

## Ожидание / приёмка

Селектор принимает и правильно раскрывает hex-эскейп (1–6 цифр + один необязательный
пробельный символ-терминатор) и identity-эскейп (`\` + любой не-hex, не-перевод-строки
символ). Регрессионный тест — таблица выше. Косвенная приёмка — вместе с BUG-1063: файлы
`shadow-dom/focus-navigation/` перестают падать на `is not a valid selector`, а `test_driver.click`
по элементу с `id` доходит до исполнителя. Baseline `tests/wpt/metadata/**` в затронутых
категориях после починки перегенерировать (`--update-expected` + три `--check`), отклонения
`--check` до этого момента — ожидаемые, а не регрессия.

## Срез 44 WPT-RUN-7 (2026-09-21, `pointerevents`)

Эта ветка (`#2 = …`, hex-эскейп каждого символа `id`) дала **152 из 258 id** категории `pointerevents` — самая массовая причина harness-`ERROR` из всех категорий, снятых до сих пор (в `shadow-dom` видна была только `*|`-ветка, [BUG-1063](BUG-1063-OPEN.md): здесь 12 id). Обе ветки вместе — 164 из 258. Baseline записан с `expected: ERROR`; после фикса регенерировать в том же или следующем коммите.

## Срез 47 WPT-RUN-7 (2026-09-21, `editing`)

`editing`: **13 id из 700** — `eval: JS runtime error: #\66 \69 \72 \73 \74  is not a valid selector` и аналоги (`#\73 \65 \63 \6f \6e \64 `, `#\70 \61 \64 \64 \69 \6e \67 `,
`#\62 \6f \72 \64 \65 \72 ` …), например `/editing/other/delete-at-end-boundary-of-div-followed-by-inline-element-containing-hidden-select-element-with-non-editable-node.html`,
`/editing/other/empty-elements-insertion.html`, `/editing/run/caret-navigation-after-removing-line-break.html`. Основная масса `ERROR` в `editing` — соседний
[BUG-1063](BUG-1063-OPEN.md) (263 id); после починки обоих баги перегенерировать baseline категории.
