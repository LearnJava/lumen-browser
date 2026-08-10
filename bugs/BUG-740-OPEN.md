# BUG-740: intrinsic-ширина grid-контейнера считается как у блока

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs` —
`max_content_outer_width`, `min_content_outer_width_of_contents`,
`preferred_inline_block_width`)
**Найден:** P3 при разборе [BUG-733](BUG-733-FIXED.md), 2026-08-10

## Симптом

`display: grid` с несколькими колонками, попавший туда, где нужна
intrinsic-ширина (flex-элемент, `inline-block`, флоат, shrink-to-fit), меряется
блочным правилом «самый широкий ребёнок» вместо суммы колонок.

```html
<div style="display:flex;width:600px">
  <div style="display:grid;grid-template-columns:auto auto"><span>AAAA</span><span>BBBB</span></div>
  <div>tail</div>
</div>
```

| | Edge | Lumen |
|---|---|---|
| grid-элемент | 70.38 | 35.19 |

Тот же корень, что у [BUG-737](BUG-737-FIXED.md) (row-flex), но **не** закрыт
им: 737 сознательно ограничен flex-контейнерами, потому что честная
intrinsic-ширина grid-а требует прогона алгоритма track sizing.

## Направление фикса

CSS Grid L1 §11.5: max-content ширина grid-контейнера = сумма max-content
размеров **колонок** + `column-gap`, а не сумма всех элементов и не максимум по
ним. Значит, нужно как минимум разложить элементы по колонкам (auto-placement
уже реализован в `lay_out_grid`) и брать максимум внутри колонки. Дешёвая
аппроксимация «колонок столько, сколько в `grid-template-columns`, элементы
раскладываются по кругу» покрывает `auto auto` и `repeat(N, …)`, но врёт при
явных `grid-column`/`span` и при `repeat(auto-fill|auto-fit, …)` (число треков
там зависит от доступной ширины, которой на этапе intrinsic-расчёта ещё нет).
Правильный путь — вынести placement + track sizing из `lay_out_grid` в
переиспользуемую функцию и звать её из intrinsic-расчёта.

Ветку `_ if is_row_flex_container(b)` в трёх функциях можно расширить до
`is_row_flex_container(b) || is_grid_container(b)`, когда такая функция появится.

## Как воспроизводить

`.tmp/p3/flex-maxcontent-probe.html` в ветке `p3-bug-733-flex`, строка `#r2`
(страница печатает `getBoundingClientRect` всех участников в `<pre>`, поэтому
один и тот же файл читается и `--dump-layout` у Lumen, и скриншотом у headless
Edge).

## Дополнение 2026-08-10 (после BUG-739)

С закрытием [BUG-739](BUG-739-FIXED.md) дефект стал **видимым**, а не только
измеримым: `inline-grid` теперь получает собственный бокс с фоном и рамкой, а
его shrink-to-fit ширину по-прежнему считает блочное правило «самый широкий
ребёнок» — контейнер оказывается уже своего содержимого. Сверено с headless
Edge: `grid-template-columns: auto auto` — 42.69 против 85.38, а с явными
треками `60px 30px` контейнер выходит 10.67 при элементах на 60 и 90, то есть
фон рисуется полоской слева от содержимого. Формы лежат в `.tmp/p3/bug739.html`
(тесты 3 и 9) ветки `p3-bug-739`.
