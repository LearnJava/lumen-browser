# BUG-1548 — `list-style-position: inside`: маркер занимает не меньше `1.5em` вместо ширины своего текста, текст строки начинается правее

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/box_tree/block_flow_trampoline.rs:268-294` — `default_w = em * 1.5`, `marker_w = default_w.max(text_w)`)

## Симптом

Для `outside`-маркера ширина `max(1.5em, ширина текста)` — приемлемый запас под отступ; для `inside` маркер — обычный inline-контент строки (CSS Lists 3 §3.1, §2.4): его ширина — ширина текста (`1. `, `viii. `), а первая строка li продолжается сразу за ним. У нас короткий маркер (`i.`) занимает `24 px` при `font-size: 16px` и текст li начинается с `x=24`, у `viii.` — `x=46` при шрифте 25px; в эталонах WPT маркер — обычный текст `<span>viii. </span>viii`. 12 reftest `css-lists/content-property/marker-text-matches-*` (`list-style: <type> inside` против абзаца `1. Filler Text`), 154 `css-counter-styles`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<ol style="padding:0;list-style-position:inside;list-style-type:decimal"><li>x</li></ol>` при `font-size:16px` — ширина `::marker`, `x` начала текста | 24 px, 24 | ≈ 11,1 px (`1. `), ≈ 11,1 |
| то же при `font-size:25px`, `lower-roman`, `i` / `viii` — начало текста | 38 / 46 | ширина текста `i. ` / `viii. ` |
| `reftest_pixdiff` `lower-roman/css3-counter-styles-019.html` с `<bdi>`→`<span>` (BUG-1546 снят) — различающихся пикселей | 2 834 | 0 |

## Как найдено

WPT-RUN-14 срез 25: `css/css-lists/content-property/marker-text-matches-*` (12, у них `rel=match` стоит после `href` — `reftest_pixdiff` их не разбирает, `no-match-ref`), `css/css-counter-styles/*/css3-counter-styles-*` (остаток после замены `<bdi>`).

## Что делать

Для `inside` строить маркер как inline-контент с шириной текста; `1.5em` оставить только запасом для `outside` (и не округлять до целых — `rect` маркера `38.00`/`24.00`). Нужен также суффикс из `counter-style` (`suffix` дескриптор, по умолчанию `". "`) внутри ширины.

## Как проверить

`css/css-lists/content-property/marker-text-matches-decimal.html`, `css/css-counter-styles/lower-roman/css3-counter-styles-019.html` (после BUG-1546).
