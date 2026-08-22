# BUG-603: table presentational hints (`bgcolor`/`background`/`bordercolor`/`cellspacing`) don't apply — `bgcolor` is static-only (never re-applied on `setAttribute`), the other three are entirely unimplemented

**Статус:** FIXED 2026-08-22
**Компонент:** layout (`crates/engine/layout/src/style.rs`, `crates/engine/layout/src/selector_query.rs`)
**Найден:** P2, WPT-VENDOR-html-rendering, 2026-08-04
**Исправлен:** P1

## Симптом

```
FAIL table bgcolor attribute is correct - assert_equals: expected "rgb(255, 0, 0)" but got ""
FAIL table background attribute is correct - assert_equals: expected "url(\"...\")" but got ""
FAIL table bordercolor attribute is correct - assert_equals: expected "rgb(255, 0, 0)" but got ""
FAIL table cellspacing attribute is correct - assert_equals: expected "10px" but got ""
```
(`non-replaced-elements/tables/table-attribute.html` — all 16
`{table,thead,tbody,tfoot,tr,td,th} × {background,bgcolor}` cases plus
`bordercolor`/`cellspacing` fail; by contrast the same file's `align`
(→`text-align`), `height`, `cellpadding` and `<col width>` presentational
hints on the same elements all **pass**, so this is not "table hints are
unimplemented" wholesale)

## Причина (два независимых дефекта — при повторной проверке 2026-08-22 остался живым только один)

1. **Заявленный дефект «`bgcolor` применяется только из начального парсинга,
   никогда не переприменяется на рантайменый `setAttribute`» не
   воспроизводится на текущем `main`.** Живая проба `--mcp-live-port` с той
   же конструкцией, что в исходном отчёте (`<table id=t>` без `bgcolor` в
   разметке → `t.setAttribute('bgcolor', 'red')` после загрузки страницы),
   даёт `getComputedStyle(t).backgroundColor === "rgb(255, 0, 0)"` —
   корректно, не `"rgba(0, 0, 0, 0)"`, как описывал отчёт. Вероятная причина
   расхождения: восстановление стилей после мутации атрибутов со страницы
   (`NodeChange::Unattributed` → безусловное расширение до родителя,
   `restyle_root_set_for_node_change`, BUG-341 S17) прилетело в main уже
   *после* того, как этот баг был заведён 2026-08-04, и попутно закрыло этот
   дефект как побочный эффект — не было отдельного коммита с формулировкой
   «чинит BUG-603 пункт 1». `apply_bgcolor_presentational_hint` сама по себе
   не имеет условий, ограничивающих её начальным проходом — вызывается на
   каждом `compute_style`, как и остальные presentational-hint функции этого
   файла.

2. **`background` (image), `bordercolor`, `cellspacing` не имели
   presentational-hint реализации** — исправлено этим коммитом:
   `apply_background_image_presentational_hint`,
   `apply_bordercolor_presentational_hint`,
   `apply_cellspacing_presentational_hint` (`style.rs`), плюс
   `computed_style_to_map` (`selector_query.rs`) научился сериализовать
   `background-image`/`border-color`/`border-spacing` для
   `getComputedStyle().getPropertyValue(...)` — этих трёх ключей раньше не
   было в карте вовсе. Проверено живой пробой (`--mcp-live-port`,
   `<table bgcolor background bordercolor cellspacing>`, все четыре атрибута
   выставлены через `setAttribute` после загрузки): все четыре значения
   применяются и корректно читаются, как статически, так и динамически.

## Масштаб

`bgcolor`/`color`/`font-size` presentational hints (BUG-021,
`apply_text_color_presentational_hint`, `apply_font_element_presentational_hints`)
были заподозрены в том же классе дефекта — измерение 2026-08-22 показало,
что общий механизм инвалидации (BUG-341 S17) уже покрывает любую
атрибут-мутацию со страницы одинаково (widen-to-parent, независимо от имени
атрибута), так что предположение не подтвердилось: отдельного починки не
требуется.

## Проверка

Живые пробы `--mcp-live-port` (не сохранены в репозитории — временные
`.tmp/probe_bug603_*.py`), плюс юнит-тесты в `style.rs`/`selector_query.rs`
(`background_hint_*`, `bordercolor_hint_*`, `cellspacing_hint_*`,
`computed_map_background_image_*`, `computed_map_border_color_*`,
`computed_map_border_spacing_*`) — 45+10 тестов, все проходят
(`cargo test -p lumen-layout`, 3603 unit + 71 integration passed).
`cargo test -p lumen-paint` (1014 passed) — display-list снапшоты не
дрейфовали, ни одна из существующих графических тест-страниц не использует
эти легаси-атрибуты.
