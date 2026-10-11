# BUG-1601 — CSS Syntax: сериализация `An+B` (`n`, `-n+3`), диапазоны `urange`, `inclusive-ranges`, `serialize-consecutive-tokens`

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** css-parser (`crates/engine/css-parser/src/` — разбор и `selectorText`-сериализация `:nth-*()`; токенизатор CSS Syntax 3)

## Симптом

`insertRule("li:nth-child(n){}")` → `selectorText` `li:nth-child(1n)` (ожидается `li:nth-child(n)`); `-n+3` → `-1n+3`; `+n` → `1n`; `N` → `1n`. Спецификация (CSS Syntax 3 §6 `An+B`, Selectors 4 §14.4 сериализация) требует минимальной формы без `1`. Проба: `.tmp/s28/g1.html`, `--dump-layout`, шесть селекторов.

## Охват

WPT-RUN-14 S28: `css/css-syntax` — 47 id, зелёных 24; не зелёных 23: `anb-serialization` (20 из 20 подтестов), `anb-parsing` (35 из 67), `inclusive-ranges` (28 из 38; `expected "parse error" but got "foo"`), `urange-parsing` (79 из 95), `serialize-consecutive-tokens` (69 из 72), `ident-three-code-points`, `decimal-points-in-numbers`, `unclosed-constructs`, `non-ascii-codepoints` (ERROR `Cannot read properties of undefined (reading '0')`), `at-rule-in-declaration-list`, 5 `charset/*`. Причина установлена пробой только для `An+B`; остальное — по именам и сообщениям.

## Как проверить

`css/css-syntax/anb-serialization.html`, `anb-parsing.html`, `urange-parsing.html`, `inclusive-ranges.html`.
