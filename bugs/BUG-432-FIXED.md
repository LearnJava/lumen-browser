# BUG-432

**Статус:** FIXED 2026-08-23
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

## Исправление (2026-08-23, P3)

Два места.

**layout** (`box_tree.rs::split_first_line_boxes`) — бокс первой строки теперь
получает роль `BoxRole::Pseudo(PseudoKind::FirstLine)`, в обеих ветках: и при
разрезе многострочного рана, и при рестайле односрочного на месте. Вариант
`PseudoKind::FirstLine` существовал и его doc-комментарий уже обещал, что его
выставляет `split_first_line_boxes`, — но не выставлял никто: `grep` по
`crates/` не находил ни одного места конструирования.

**paint** (`display_list.rs::emit_first_line_background`) — по этой роли
`emit_inline_run` красит фон псевдоэлемента перед текстом. Заодно `dpr`
протянут в `emit_inline_run` (три вызова), чтобы градиентный фон уходил в
`emit_background_image` с настоящим dpr, а не с подставленной единицей.

**Экстент — не полная ширина бокса.** CSS Pseudo-elements L4 §4.1: псевдоэлемент
ведёт себя как фиктивный inline-тег, оборачивающий содержимое строки, поэтому
красится объединение фрагментов строки. У самого бокса `rect.width` — ширина
контейнера (300px в пробе), а фон должен кончаться на последнем слове (289px).
Box-model свойства (margin/padding/border) к `::first-line` не применяются и
здесь не рисуются.

**Заявленный риск не материализовался.** Ключ — роль, а не стиль, так что
анонимные раны не задеты по построению; сверх того их строит `anon_style`,
который обнуляет `background_color`, паддинги и рамки. Проверено пробой той же
формы, что и обе страницы графтестов с `::first-line` (фон у родителя,
у псевдоэлемента только `color`): ровно один `FillRect`, не два.

### Проверка

- Проба бага: `FillRect (8, 8, 289, 19) #333333ff` под первой строкой,
  до `DrawText`. До фикса — ни одного `FillRect`.
- Краевые случаи одной пробой: односрочный абзац (ветка «рестайл на месте»),
  абзац без правила `::first-line` (лишних команд нет), `<b>` со своим фоном
  внутри первой строки (фон псевдоэлемента идёт ПОД фоном `<b>`),
  `text-align: right` (объединение едет за текстом, x=150), градиент
  (`DrawLinearGradient` шириной 197, а не 300).
- Два юнит-теста в `display_list.rs`: `first_line_background_paints_under_first_line`
  и обратный `inline_run_without_first_line_rule_paints_no_background`.
- `cargo test -p lumen-paint` (1047) и `-p lumen-layout` (3675) зелёные,
  clippy обоих крейтов чистый.
- Нейтральность на существующем контенте: `dump_golden.py` 12/12 совпадают,
  все 81 детерминированный CPU-снимок совпадают, TEST-58 (единственная
  автодифф-страница с `::first-line`) 2.39% против baseline 2.47%.
  `1000000-final.html` — страница ручной проверки, в автодифф не входит.
