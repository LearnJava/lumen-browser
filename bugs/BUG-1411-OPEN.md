# BUG-1411 — Строка, состоящая только из `<br>` или `&nbsp;`, не занимает высоты; `<br>` в начале и подряд теряет перевод строки

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_build.rs` / `inline_wrap.rs` — построение строки; кандидат: тот же `split_whitespace()`/`trim`, что в BUG-1323)

## Симптом

`--dump-layout`, `body{margin:0;line-height:20px;font-size:16px}`, высота `<div id=t>` с содержимым:

| содержимое | получено | ожидается |
|---|---|---|
| `<br>` | **0** | 20 |
| `&nbsp;` (`<p>`) | **0** | 20 |
| `<br>D` | **20** | 40 |
| `B<br><br>` | **20** | 40 |
| `<br><br>` | **0** | 40 |
| `<span><br></span>` | **0** | 20 |
| `<br>&nbsp;` | **0** | 40 |
| `B<br>` | 20 | 20 |
| `F<br>G` | 40 | 40 |
| `&#8203;` (U+200B) | 19,36 | строка есть |

`--screenshot` с `background:red` на `<div>` даёт ту же высоту: `<br>D` — 20, `x<br><br>` — 20, `<br>` — 0,
`x<br>y` — 40. Блок, который рендерится как пустой и сжимается в 0, — признак того, что строка из одних «пустых»
узлов отбрасывается ещё при построении, а ведущий/подряд идущий `<br>` не порождает пустую строку.

`<p>&nbsp;</p>` в эталоне `font-size-zero-2-ref.html` даёт 0, в тесте `<p><span style="font-size:0">zero</span></p>` —
19,2 (strut от `line-height: 1.2`): тест и эталон расходятся из-за этого дефекта, а не из-за `font-size: 0`.

## Как найдено

WPT-RUN-14 срез 18: `css-fonts/font-size-zero-2.html`; 23 упавших reftest срезов с `<br>`
(`font-synthesis-*-first-line.html`, `font-weight-search-direction.html`, `italic-oblique-fallback.html`,
`size-adjust.tentative.html`, …). Влияние на WPT не измерено: часть этих id падает и по BUG-1273.

## Что делать

Локализовать место, где строка из одних `<br>`/NBSP отбрасывается (`inline_build.rs`/`inline_wrap.rs`, кандидат —
`split_whitespace()` и `trim()`, считающие NBSP пробелом, как в [BUG-1323](BUG-1323-FIXED.md)); `<br>` обязан порождать
конец строки и пустую строку, если перед ним нет текста; NBSP не схлопывается и держит строку (CSS Text L3 §4.1).

## Как проверить

Проба выше; `css/css-fonts/font-size-zero-2.html`.
