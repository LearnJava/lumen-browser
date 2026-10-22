# BUG-1686 — выделение не подсвечивает текст внутри shadow-дерева

**Статус:** OPEN
**Заведён:** 2026-10-22 (P2, WPT-RUN-16 — разбор падения категории `selection` −8.00 балла)
**Область:** layout (`crates/engine/layout/src/highlight.rs` — `TreeOrder`)

## Симптом

`getSelection().selectAllChildren(document.body)` на странице с shadow-хостом подсвечивает только
light-DOM текст; текст из shadow root хоста остаётся без фона `::selection`. В эталоне тот же текст
лежит обычным `<div>` и подсвечен.

Репро — `tests/wpt/selection/shadow-dom/cross-shadow-boundary-1.html` против `-1-ref.html`:

```
lumen --viewport 400x120 --screenshot test.png tests/wpt/selection/shadow-dom/cross-shadow-boundary-1.html
lumen --viewport 400x120 --screenshot ref.png  tests/wpt/selection/shadow-dom/cross-shadow-boundary-1-ref.html
```

В тесте подсвечен только «OuterText», «innerText» (в shadow root) — нет; в эталоне подсвечены обе строки.

## Почему

`TreeOrder::new` (`highlight.rs:162`) обходит только `doc.get(node).children`; дети shadow root в обход
не попадают, поэтому `node_spans` не находит их текстовые узлы между концами диапазона. До
HIGHLIGHT-PAINT (`cfa28ab9f`) выделение в растр не попадало вовсе, тест и эталон рисовались одинаково,
и 9 рефтестов `cross-shadow-boundary-*` были ложным PASS. В прогоне 2026-10-22 все 9 — FAIL.

Попутно видно (и в тесте, и в эталоне): фон выделения короче строки — у «OuterText» не закрыта
последняя буква. На PASS рефтеста не влияет (одинаково с обеих сторон), но это отдельный дефект ширины.

## Как проверить

`tests/wpt/run_corpus.py --prefixes selection/shadow-dom` — `cross-shadow-boundary-1…6`, `-img`,
`-select-root`, `-select-document` зелёные (было 9 FAIL, категория `selection` 69.41 → ~77).
