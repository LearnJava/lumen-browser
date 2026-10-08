# BUG-1327 — пробельный текстовый узел между inline-элементами пропадает в `white-space: pre|pre-wrap|break-spaces`; U+3000 между элементами схлопывается как пробел

**Статус:** FIXED 2026-10-08 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_build.rs:148` `is_inline_content` и `box_tree/build.rs:1249` — текст, целиком состоящий из `char::is_whitespace()`, считается «пустым» и пропускается независимо от `white-space`)

## Симптом

`--dump-layout`, `font: 20px monospace; white-space: pre`, ширина пробела 11 px, `x` фрагмента `b`:

| разметка | получено | ожидается |
|---|---|---|
| `<b>a</b>    <b>b</b>` (4 пробела) | 11 (пробелы потеряны) | 55 |
| `<b>a</b><i>    </i><b>b</b>` | 11 / 54,98 (верно — пробелы внутри элемента сохраняются) | 55 |
| `a    <b>b</b>`, `<b>a</b>    b` | верно (пробелы в узле с непробельным текстом) | — |
| `<b>a</b>U+3000 U+3000<b>b</b>`, `white-space: pre` | 11 | 33 |
| `<b>a</b>U+3000<b>b</b>`, `white-space: normal` | `a b` (U+3000 → U+0020) | пробел-иероглиф шириной 1em |

`display:inline-block` с `<b>a</b>    <b>a</b>` в `pre` — ширина 22,46 (как без пробелов), с `<b>a</b>x    <b>a</b>` — 55,88. Узел из одних пробелов (U+0020, U+0009, U+3000…) между двумя inline-элементами отбрасывается целиком: ни сохранение пробелов (`pre`/`pre-wrap`/`break-spaces`), ни неколлапсируемый U+3000 до построения сегментов не доходят.

## Как найдено

WPT-RUN-14 срез 10: `white-space/break-spaces-newline-011/014/015/016.html`, `pre-wrap-018.html` (reftest), `white-space-intrinsic-size-021.html` (168 сабтестов).

## Проверка гипотезы (A/B, не закоммичено)

Временная ветка `LUMEN_AB_PRE` в обоих местах (пропускать узел только если `!preserves_whitespace()`), один бинарь, `run_corpus.py --prefixes css/css-text/white-space,css/css-text/word-break,css/css-text/text-transform`: 218 → 223 зелёных id из 672. `break-spaces-newline-011/014/015/016` и `pre-wrap-018` FAIL → PASS, `white-space-intrinsic-size-021` 24 → 26 из 192 сабтестов, новых провалов нет. Правка в коммит не вошла (срез — прогон, движок не менялся).

## Что делать

Не пропускать пробельный узел, когда `white-space` сохраняет пробелы, и никогда — когда в нём есть символы вне `{U+0020, U+0009, U+000A, U+000C, U+000D}` (CSS Text L3 §4.1.1; готовый предикат — `is_collapsible_whitespace`, `box_tree/entry.rs:911`). Заменить `char::is_whitespace()` на него в двух местах (+ ветка `pre-line`, `inline_build.rs:926`).

## Как проверить

`css/css-text/white-space/break-spaces-newline-011.html`, `pre-wrap-018.html`; проба выше.

## Причина и исправление

Три места решали «текстовый узел пуст» через `char::is_whitespace()` / `is_wrap_whitespace`, не глядя на `white-space`: `is_inline_content` (`inline_build.rs`), группа anonymous-блока (`build.rs`) и главный цикл сборки inline-контекста (`build.rs`). Теперь все три зовут `is_discardable_text(текст, white_space)` (`box_tree/entry.rs`): узел отбрасывается, только если состоит из невидимых управляющих и пробелов, которые свойство схлопнет — `pre`/`pre-wrap`/`break-spaces` не схлопывают ничего, `pre-line` сохраняет `
`, а символы вне набора `{U+0020, U+0009, U+000A, U+000C, U+000D}` (U+3000…) — содержимое при любом `white-space`. Ветка `pre-line` в `collect_inline_segments` тоже сравнивает с `is_collapsible_whitespace`, а не с `is_wrap_whitespace`.

Тесты: `box_tree/tests/ws_only_text.rs` (4 штуки: пробелы между элементами в `pre`/`pre-wrap`/`break-spaces`, схлопывание в `normal`/`nowrap`/`pre-line`, `
` между элементами в `pre`/`pre-wrap`/`pre-line`, U+3000 в `pre`).

**Остаток — [BUG-1462](BUG-1462-OPEN.md):** в `white-space: normal` U+3000 всё ещё сводится к U+0020 при разбиении на слова (последняя строка таблицы симптомов). WPT-прогон `css/css-text/white-space` после правки не делался (нужен WPT-venv).
