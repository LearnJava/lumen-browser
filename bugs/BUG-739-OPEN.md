# BUG-739: `display: inline-flex` / `inline-grid` не создают бокс вообще

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs` — построение
дерева боксов, `collect_inline_segments` / выбор формат-контекста)
**Найден:** P3 при разборе [BUG-733](BUG-733-OPEN.md), 2026-08-10

## Симптом

Элемент с `display: inline-flex` или `display: inline-grid` не получает
собственного бокса: его содержимое уплощается в `InlineRun` родителя, как если
бы `display` был `inline`. Не рисуются ни фон, ни рамка; flex/grid-алгоритм не
запускается; ширины/высоты элемента нет.

`--dump-layout` по странице

```html
<div><div style="display:inline-flex;background:#ccf"><span>AAAA</span><span>BBBB</span></div></div>
```

даёт один `InlineRun` с сегментами `"AAAA"`, `"BBBB"` и ни одного бокса с фоном.
Так же ведут себя `<span style="display:inline-flex">` и
`display: inline-grid` (`grid-template-columns` игнорируется целиком). Соседний
`display: inline-block` работает штатно — то есть проблема не в inline-уровне
как таковом, а в том, что именно эти два значения нигде не блокифицируются в
atomic inline-level бокс.

По спеке (CSS Display L3 §2.1) `inline-flex`/`inline-grid` — atomic inline-level
боксы: снаружи ведут себя как `inline`, внутри создают flex/grid formatting
context, то есть должны собираться в тот же `BoxKind::InlineBlockRow`, что и
`inline-block`, но лэйаутиться `lay_out_flex`/`lay_out_grid`.

При этом `CSS-SPECS.md` объявляет `display` ✅ со списком значений, включающим
`inline-flex`/`inline-grid`: парсинг и `ComputedStyle` действительно есть
(`Display::InlineFlex`/`InlineGrid`), не реализован именно layout.

## Влияние

Прямой вклад в пункт 2 [BUG-733](BUG-733-OPEN.md) — схлопнувшуюся CTA-кнопку
`tbank.ru`: её содержимое (`span.dbwTheaUM`, `span.abErxfKrf`,
`span.cbErxfKrf { width: 16px }` — иконка) объявлено `display: inline-flex`,
поэтому боксов не имеет и ширины кнопке не даёт. Пункт 2 закрывается только
вместе со вторым дефектом — `width: 100%` внутри inline-escape
(см. заметку в [BUG-733](BUG-733-OPEN.md)).

## Как воспроизводить

`.tmp/p3/inline-flex.html` и `.tmp/p3/inline-grid.html` в ветке
`p3-bug-733-flex` (обе страницы читаются `--dump-layout` напрямую).

## Направление фикса

Блокификация в атомарный inline-level бокс: там, где `display: inline-block`
кладётся в `InlineBlockRow`, тот же путь должен принимать `InlineFlex`/
`InlineGrid`, а внутренний лэйаут диспетчеризоваться в `lay_out_flex` /
`lay_out_grid` (диспетчер уже умеет оба значения — см. ветки
`Display::Flex | Display::InlineFlex` в `lay_out`). Intrinsic-ширина
`inline-flex` после [BUG-737](BUG-737-FIXED.md) считается правильно
(`is_row_flex_container` учитывает `InlineFlex`), отдельной работы не требует.
