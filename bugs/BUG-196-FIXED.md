# BUG-196

**Статус:** FIXED 2026-06-18
**Компонент:** css-parser/layout
**Тест:** TEST-67 (16.41% → 1.36%, KNOWN_DEBTORS)

## Описание

`content: attr(data-label)` на `.swatch::before` (с `display: flex`) генерирует
тёмные label-боксы перед каждым баром. На странице эти `::before` вообще не
рисовались — все пять тёмных боксов отсутствовали, бары съезжали влево на 200px.

## Корень

Парсинг `attr()` (`parse_content_fn`) и его резолв в текст
(`content_to_inline_segments`) уже работали. Проблема была в том, что
`::before`/`::after` инжектились ТОЛЬКО для `BoxKind::Block | FlowRoot`
(`build_box`, ветка после non-item-container). Для flex/grid-контейнеров
(`is_item_container`) псевдоэлементы не создавались вовсе. `.swatch` —
`display: flex`, поэтому её `::before` тихо терялся.

## Фикс

1. `inject_pseudo` получил параметр `blockify: bool` — при `true` каждый
   псевдоэлемент кладётся в собственный block-level бокс независимо от его
   `display` (CSS Flexbox §4 / Grid §6: in-flow дети flex/grid-контейнера
   блокифицируются в отдельные items, нельзя мерджить в соседний InlineRun).
2. В ветке `is_item_container` (`build_box`) для `Flex`/`InlineFlex`/`Grid`/
   `InlineGrid` теперь вызывается `inject_pseudo(..., blockify=true)` для
   `before` и `after`. Таблицы исключены (свои anonymous-box правила).

`dump-layout` подтвердил: тёмный бокс `#2c3e50` 200×60 генерируется перед
баром, бар сдвинут на x=241, attr-текст «Width: …» резолвится.

## Регресс-тесты

- `flex_container_before_pseudo_generates_item` — flex `::before` с
  `content:attr()` создаёт blockified flex item с фоном и текстом.
- `flex_container_without_before_has_no_extra_item` — без правила `::before`
  фантомный item не появляется.

## Остаток

1.36% > 0.5% — чистый font-parity (белый monospace-текст label'ов, Inter vs
Edge) + sub-pixel edge-AA по `border-radius` клипу. Тёмные боксы и цветные
бары совпадают с Edge пиксель-в-пиксель (см. diff). TEST-67 → KNOWN_DEBTORS
(BUG-128, baseline 1.36%).
