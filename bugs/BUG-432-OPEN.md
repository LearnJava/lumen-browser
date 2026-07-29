# BUG-432

**Статус:** OPEN
**Компонент:** paint (`crates/engine/paint/src/display_list.rs`)
**Файл:** `display_list.rs` — ветка `BoxKind::InlineRun` в `walk`

## Описание

Фон (и любое другое box-level оформление) псевдоэлемента `::first-line` не
красится: `walk` для `BoxKind::InlineRun` зовёт только `emit_inline_run`,
который рисует текст, — у InlineRun-бокса нет собственной покраски фона /
рамки / тени, в отличие от `emit_box_self` у блоков.

Между тем layout честно кладёт стиль псевдоэлемента на этот бокс:
`split_first_line_boxes` (`box_tree.rs`) делает
`child.style = Arc::new(*fls)`, и её doc-комментарий прямо обещает
«background, text-decoration, color and font all take effect at paint time» —
для фона это неправда.

CSS Pseudo-elements L4 §3.2 относит все background-свойства к применимым
к `::first-line`.

## Проба

```html
<style>p{width:300px} p::first-line{background:#333}</style>
<p>Delta beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi</p>
```

`--dump-display-list` даёт только `FillRect` фона body; ни одного `FillRect`
под первую строку нет.

## Ожидание

`FillRect` шириной по line box первой строки, под её текстом.

## Заметки

Найдено 2026-07-29 при разборе [BUG-100](BUG-100-OPEN.md) (наследование
сквозь фиктивную обёртку `::first-line`). В том баг-фиксе не трогалось,
чтобы не мешать layout-правку с покраской.

Риск при фиксе: покраска фона у `BoxKind::InlineRun` в общем виде затронет
все анонимные inline-раны. `background-color` не наследуется, поэтому у
анонимных ранов он `None` — но проверить это надо полным прогоном
graphic_tests, а не рассуждением.
