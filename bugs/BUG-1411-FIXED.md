# BUG-1411 — Строка, состоящая только из `<br>` или `&nbsp;`, не занимает высоты; `<br>` в начале и подряд теряет перевод строки

**Статус:** FIXED 2026-10-26 (P6)
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

## Влияние на SHAPE-OUTSIDE-2 (2026-10-21)

42 reftest `css-shapes/shape-outside` с `<br/>` после float (`supported-shapes/circle/shape-outside-circle-026.html` и др.) не проходят только из-за этого дефекта: подмена `<br/>` на блок высотой 40px даёт верный контур. Проба: `<div style="font:40px/1 Ahem;width:300px"><br/><br/>X</div>` — два пустых `Block` высотой 0 и один `InlineRun` с `X` (должно быть три строки).

## Исправление (2026-10-26, P6)

Корень: `<br>` в UA получает `display: block` (`style/ua.rs`), поэтому бокс-дерево режет поток на `InlineRun` вокруг нулевого по высоте `Block`. `B<br>` при этом верно (строку закрывает `<br>`), но `<br>`, перед которым нет текста (ведущий, подряд, после блока), обязан открыть пустую строку — её не было. `&nbsp;` в `<p>` уже давало строку (проба в `--dump-layout` на сборке 2026-10-26: 20).

Правка: `give_leading_breaks_a_line` (`box_tree/inline_build.rs`, вызов из `build.rs` в ветке без item-контейнера) — `<br>`-блок без текста перед ним превращается в `InlineRun` из одного пустого forced-break сегмента; `wrap_inline_run` даёт ему пустую строку высотой со strut. Float/abspos/маркер строку не «держат»; во flex/grid `<br>` не трогается. Тесты — `tests/br_lines.rs`; проба `--dump-layout` по всем вариантам таблицы выше даёт 20/20/40/40/40/20/40/20/40.

Остаток: 42 reftest `shape-outside` с `<br/>` после float (SHAPE-OUTSIDE-3) не перемерялись.
