# BUG-1668 — `margin: auto` не центрирует блок, ширину которого ограничил `max-width`

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), example.com)
**Область:** layout (ширина блока в потоке: `max-width` применяется после того, как auto-поля уже получили 0)

## Симптом

```html
<!doctype html><div id=d style="max-width:400px;margin:0 auto">x</div>
```

viewport 1280: Chrome `#d.getBoundingClientRect()` = `440,8,400,18`, Lumen — `8,8,400,19.4`.
`--dump-layout`: `Block rect=(8.00, 8.00, 400.00, 17.72) max-w=400.00 m=(0.00, auto, 0.00, auto)` —
ширина обрезана до 400, а свободные 864px не ушли в auto-поля. С `width:400px` вместо `max-width`
блок центрируется верно.

На реальных сайтах — главный приём центрирования колонки: example.com (`body{max-width:26em;margin:auto}`,
body у Lumen с `x=0` против 432 у Chrome; весь текст страницы прижат влево).

## Что делать

CSS 2.2 §10.4: если вычисленная ширина больше `max-width`, правила §10.3.3 применяются заново с
`width = max-width` — то есть auto-поля пересчитываются от новой ширины. Найти место, где блок в потоке
получает used width (block flow), и пересчитывать auto-поля после клампа `max-width`/`min-width`.

## Как проверить

Репро выше: `x = 440`. `python scripts/geom_diff.py --only example` — `body` без Δx.
WPT: `css/CSS2/margin-padding-clear/margin-auto-*`, `css/css-sizing/` с `max-width` + `margin:auto`.
