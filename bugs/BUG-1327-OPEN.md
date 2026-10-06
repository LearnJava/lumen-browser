# BUG-1327 — пробельный текстовый узел между inline-элементами пропадает в `white-space: pre|pre-wrap|break-spaces`; U+3000 между элементами схлопывается как пробел

**Статус:** OPEN
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
