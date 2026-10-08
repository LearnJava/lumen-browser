# BUG-1416 — Цикл в `attr()` (значение атрибута содержит `attr(` на себя же) — бесконечная рекурсия, переполнение стека

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout (`crates/engine/layout/src/style/substitute.rs::expand_attr_val`)

## Симптом

`--dump-layout` на странице из одного элемента (код возврата процесса):

| разметка | результат |
|---|---|
| `<div data-foo="attr(data-foo type(<length>))" style="--x:attr(data-foo type(<length>))">` | **`0xc00000fd`** — переполнение стека потока `main` |
| то же с `style="width:attr(data-foo type(<length>))"` | `0xc00000fd` |
| `data-foo="attr(data-foo type(*))"`, `style="--y:attr(data-foo type(*))"` | `0xc00000fd` |
| `data-foo="attr(data-foo type(<length>), 11px)"`, `--x:attr(data-foo type(<length>))` | `0xc00000fd` |
| `data-foo="x"`, `--x:attr(data-foo type(*))` (контроль, без цикла) | 0 |
| `data-foo="attr(data-foo)"`, `--x:attr(data-foo type(<ident>))` | 0 |

Под `wptrunner` (`run_corpus.py --prefixes css/css-values/attr-cycle.html,css/css-values/if-cycle.html`): оба — CRASH, в логе
`thread 'lumen-v8' has overflowed its stack`.

Двоичный поиск по строкам `attr-cycle.html` (страница, у которой `test()` заменён заглушкой): первая падающая строка —
`test_attr_cycle('--x', 'attr(data-foo type(<length>))', 'attr(data-foo type(<length>))')` (строка 38).

## Причина (код, не проба)

`expand_attr_val` (`substitute.rs:1004–1052`): после подстановки `resolved` собирает `combined` и, если там есть `attr(`,
вызывает себя же (`substitute.rs:1044`). Атрибут, значение которого содержит `attr(<тот же атрибут>)`, даёт бесконечную
рекурсию. CSS Values 5 §attr(): цикл делает декларацию «invalid at computed-value time» (значение — `unset`/начальное).

## Как найдено

WPT-RUN-14 срез 19: `css-values/attr-cycle.html` — CRASH; `if-cycle.html` — тоже CRASH, но `--dump-layout` страницы (и
урезанных страниц с `if()` в `--prop`) завершается кодом 0, то есть цикл `if()` под `wptrunner` не локализован (возможно,
другой путь — вычисление `getComputedStyle` из JS).

## Что делать

Ограничить глубину подстановки (счётчик/набор «имя атрибута уже раскрывается») и вернуть `None` (невалидно при вычислении)
при повторе. Затем проверить `if()` под `wptrunner`.

## Как проверить

`css/css-values/attr-cycle.html` и `if-cycle.html`: оба не CRASH; страница из таблицы выше — код 0.
