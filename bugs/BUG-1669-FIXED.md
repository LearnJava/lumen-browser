# BUG-1669 — `<center>` не центрирует блочного потомка (таблицу)

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), news.ycombinator.com)
**Область:** layout/UA-стили (`center { text-align: -webkit-center }` — выравнивание блочных потомков, HTML Rendering §15.3.3)

## Симптом

```html
<center><table id=t style="width:50%"><tr><td>x</td></tr></table></center>
```

viewport 1280, и в quirks, и в standards режиме: Chrome `#t` = `324,8,632,24`, Lumen — `8,8,632,21.4`.
Таблица получает ширину, но стоит у левого края.

HN (`<center><table id="hnmain" width="85%">`) — вся лента у Lumen съехала влево на 93px
(`body>center:1>table:1` Δx −93.5), и за ней каждый из 264 корней геометрии страницы.

## Причина

У `<center>` не было UA-правила вовсе, а presentational hint `align=center` давал обычный
`text-align: center`. По HTML Rendering §15.3.3 `<center>` и `align=center` — это
`text-align: -webkit-center`: он не только выравнивает строчное содержимое, но и центрирует
блочных потомков с ограниченной шириной (как `justify-self: center` для margin box).
В движке такого значения не было: `align_justify_self_child` центрит блок только по
явному `justify-self`, а `normal` оставлял у начала строки.

## Что сделано

- `TextAlign::WebkitCenter` (`style/values/typography.rs`): разбор `-webkit-center`,
  сериализация в computed как `-webkit-center`; для строк ведёт себя как `center`
  (`inline_wrap::align_one_line`, ряд `inline-block` в `dispatch_box.rs`).
- UA-правило `<center>` → `-webkit-center` (`ua_text_align`, применяется до каскада);
  presentational hint `align=center`/`middle` на `div`/`p`/`h1`–`h6`/… → `-webkit-center`.
- `block_flow_trampoline::webkit_center_alignment`: блочный потомок (включая
  `BoxKind::Table`), у которого `justify-self`/`justify-items` остаются `auto`/`normal`,
  центрится по `-webkit-center`; явное значение (`start`, `stretch`, …) по-прежнему
  побеждает. Plain `text-align: center` блочных детей не трогает.

## Проверка

- `cargo test -p lumen-layout` — 6021 passed, в том числе новые
  `box_tree::tests::bug1669_webkit_center` (`<center>` и `div align=center`: таблица 400px
  в 800px → `x=200`; plain `text-align: center` → `x=0`; вложенный блок; явный
  `justify-self: start` побеждает) и `selector_query::webkit_center_is_serialised`.
- `cargo clippy -p lumen-layout --all-targets -- -D warnings` — чисто.
- `python graphic_tests/dump_golden.py` — 12/12 совпадают с эталоном.
- Полный `python graphic_tests/run.py` в этой сессии недоступен: gdigrab не находит
  magenta-маркер (TEST-00), поэтому прогон сделан с `--ipc`. Набор падений совпал с последним
  полным прогоном на `main` (24 FAIL: 17 — GPU_CAPTURE, которым нужен gdigrab, остальные —
  известные дефекты, `61` — BUG-1697); ни одна падающая страница не содержит `center`,
  `align=center` или `-webkit-center`.

## Живая проверка

`python scripts/geom_diff.py --only hn` — у `body>center:1>table:1` больше нет Δx.
