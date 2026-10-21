# BUG-1669 — `<center>` не центрирует блочного потомка (таблицу)

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), news.ycombinator.com)
**Область:** layout/UA-стили (`center { text-align: -webkit-center }` — выравнивание блочных потомков, HTML §15.3.3)

## Симптом

```html
<center><table id=t style="width:50%"><tr><td>x</td></tr></table></center>
```

viewport 1280, и в quirks, и в standards режиме: Chrome `#t` = `324,8,632,24`, Lumen — `8,8,632,21.4`.
Таблица получает ширину, но стоит у левого края.

HN (`<center><table id="hnmain" width="85%">`) — вся лента у Lumen съехала влево на 93px
(`body>center:1>table:1` Δx −93.5), и за ней каждый из 264 корней геометрии страницы.

## Что делать

HTML §15.3.3 (rendering, `center` и `align` у `div`/`p`/`td`…): `center` — `text-align: -webkit-center`
(в спецификации — «как `text-align: center`, плюс блочные потомки центрируются auto-полями»).
Нужно, чтобы унаследованное `-webkit-center` (и `align=center` у `div`/`td`) сдвигало блочных потомков
с явной/ограниченной шириной в середину, как у Chrome. Проверить и `<div align=center><table>`.

## Как проверить

Репро выше: `x = 324`. `python scripts/geom_diff.py --only hn` — у `body>center:1>table:1` нет Δx.
WPT: `html/rendering/non-replaced-elements/flow-content-0/center-*`, `div-align.html`.
