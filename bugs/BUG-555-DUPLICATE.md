# BUG-555: `getComputedStyle()` returns an empty string for every property when queried from a synchronous inline `<script>` that runs during the initial HTML parse (before the page's first layout snapshot is published) — residual case of BUG-382

**Статус:** DUPLICATE → [BUG-443](BUG-443-FIXED.md)
**Дата:** 2026-08-04
**Компонент:** shell (`crates/shell/src/page_pipeline.rs::apply_loaded_page` and the
`update_layout_rects`/`update_computed_styles` publish call inside it), js
(`crates/js/src/dom.rs:12769` `window.getComputedStyle`) — same subsystem as
[BUG-382](BUG-382-FIXED.md)
**Найден:** P2, WPT-RUN-3 срез 38 (`css/css-layout-api`), 2026-08-04
**Закрыт как дубликат:** P3, 2026-09-13

## Ревизия P3 2026-09-13

Заявка (2026-08-04) описывает ровно тот же механизм, что [BUG-443](BUG-443-FIXED.md)
(найден P1 2026-07-29, исправлен P3 2026-08-30 — **после** подачи этой заявки, но
без пометки её как закрытой): `getComputedStyle`/`getBoundingClientRect`, прочитанные
синхронным парсинговым `<script>` или обработчиком `DOMContentLoaded`, отвечали
пустым снимком, потому что каскад и первая раскладка строились **после** исполнения
скриптов страницы. BUG-443 переставил обе фазы (`build_page_cascade` + первая
`layout_page`/`JsLayoutSnapshot`) перед `run_scripts_with_dom` и публикует снимок
сразу после `install_dom`, до первой строки кода страницы — устраняет проблему
для inline-скрипта в точности так, как эта заявка просила.

Живой регрессионный тест `crates/shell/src/tests/page_pipeline.rs::parse_time_script_reads_computed_style_and_rect`
воспроизводит дословно повторяющийся сценарий этой заявки (`<script>` сразу после
стилизованного элемента, читающий `getComputedStyle(...).width`/`.color` и
`getBoundingClientRect()` в том же тике разбора) и зелёный на актуальном `main`
(`cargo test -p lumen-shell --profile dev-release --features v8
parse_time_script_reads_computed_style_and_rect` → `ok`). Живая проверка примера
из заявки:

```html
<style>#x{color:red}</style>
<div id="x"></div>
<script>
window.__inline_color = getComputedStyle(document.getElementById('x')).color;
</script>
```

теперь детерминированно даёт `"rgb(255, 0, 0)"` вместо `""` — тот же путь публикации
снимка, что и тестовый файл выше.

## Остаток, не покрытый BUG-443 (не переносится отдельным дефектом)

`inline-style-layout-function.https.html`'s secondary finding — `element.style.display`
принимает синтаксически невалидные значения `layout()` без грамматической валидации
(Houdini custom layout, `layout()` без имени, `layout(test3, invalid)` с лишним
аргументом) — остаётся не тронутым: CSS Layout API как таковой не запланирован на
эту фазу (`CSS-SPECS.md:137`), заявка оставлена здесь как указатель для того, кто
будет его реализовывать.

## Связанные

* [BUG-443](BUG-443-FIXED.md) — реальный фикс порядка фаз (каскад/раскладка до
  скриптов), закрывает механизм этой заявки.
* [BUG-382](BUG-382-FIXED.md) — публикация снимка после первичной раскладки (для
  post-load чтений).
