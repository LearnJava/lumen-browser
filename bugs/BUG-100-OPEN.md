# BUG-100

**Статус:** OPEN (DEBTOR)
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

> **Ревизия 2026-06-23 (дрейф трекера):** ::first-letter / ::first-line **реализованы** —
> `apply_first_letter_style`, `first_line_style`/`is_first_line`, drop-cap `float:left` через
> `extract_first_letter_float` (+7 тестов). Diff-картинка TEST-58 подтверждает: фича работает;
> остаток 4.92% = font-parity тела абзаца (Inter vs Edge → разные метрики/перенос) + edge-AA
> 48px drop-cap-глифа (rule 3). Внесён в KNOWN_DEBTORS (4.92%).

## Ревизия 2026-07-29 (P3)

Заявка про остаток подтвердилась по геометрии, но под ней нашёлся отдельный
структурный дефект — не в TEST-58 (там во всех абзацах нет вложенных inline),
а в самом механизме псевдоэлементов.

### Что проверено на пикселях

Разложение `58-first-letter-line-edge.png` против нашего кадра:

- бокс `.fl-demo` совпадает с Edge ровно: y 66..146 в обоих, ширина та же;
- drop-cap флоатится и сужает **обе** строки: line 2 начинается с x=83 (Edge 87),
  разница = ширина адванса `O` у другого шрифта, а не отсутствие сужения;
- `--dump-display-list` даёт `w=700` и на drop-cap, и на `::first-line`;
  вертикальные позиции строк совпадают (line 1 y 85..99, line 2 y≈117).

Остаток целиком в глифах: диф по бэндам y — заголовок h3, две строки абзацев,
две monospace-метки, и ничего кроме. Детерминированный CPU-снапшот против Edge
даёт 2.31% (живое окно 2.55%) — то есть это не дефект какого-то одного бэкенда.
Две составляющие остатка:

1. **font-parity** (BUG-128): `sans-serif` резолвится в наш дефолтный face,
   у Edge — системный, отсюда другой перенос («great» у Edge влезает в line 1).
2. **generic-family fallback**: `resolve_face_id` (`paint/src/renderer.rs`)
   пропускает generic-имена (`serif`/`sans-serif`/`monospace`/…) и падает в тот
   же дефолтный face — `monospace`-метка рисуется пропорциональным шрифтом
   (273px против 359px у Edge на той же строке). Это Phase-0 упрощение,
   задокументированное в самом `resolve_face_id`.

Обе — не про `::first-letter`/`::first-line`, поэтому TEST-58 остаётся
KNOWN_DEBTOR-ом.

### Что починено

CSS Pseudo-elements L4 **§3.4** (Inheritance through fictional tag sequences):
`::first-line`/`::first-letter` — *родитель* затронутого содержимого, а не
сплошной override. Потомок, который сам объявил свойство (`<b>` — `font-weight`,
`<em>` — `font-style`, инлайновый `style="color:…"`), сохраняет своё значение;
от псевдоэлемента приходит только то, что потомок унаследовал.

Движок вместо этого **затирал стиль фрагмента целиком** в пяти местах:

- `apply_first_letter_pseudo` (сбор сегментов) — 2 ветки,
- `apply_first_letter_style` (пост-сплит) — 2 ветки,
- pass A подбора первой строки в `lay_out` (`fl_seg.style = fls.clone()`),
- разметка фрагментов `lines[0]` после переноса,
- `apply_first_line_pseudo_styles_inner` (пост-layout проход) — 2 ветки.

Последствия: `<p>Foxtrot <b>bold bit</b> …</p>` с `p::first-line{color}` терял
жирность `<b>` (три фрагмента схлопывались в один DrawText без `w=700`);
`<p><em>Bravo</em>…</p>` с `p::first-letter{color}` терял курсив и на самой
букве, и — в ветке split-а — на хвосте `ravo`.

Фикс: `crate::style::merge_pseudo_inherited(own, base, pseudo)` — стиль
псевдоэлемента накладывается пополю только там, где `own == base`, т.е.
свойство было унаследовано, а не объявлено. Набор полей ограничен применимыми
к этим псевдоэлементам (§3.2/§4.4) и осмысленными для текстового рана; box-level
(фон, отступы) красится боксом псевдоэлемента, не фрагментом.

Известное приближение: потомок, который *повторно объявил* значение
originating-элемента (`color: blue` внутри `color: blue`), неотличим от
наследования и проигрывает псевдоэлементу.

Побочно: хвост сегмента при split-е `::first-letter` теперь берёт собственный
стиль сегмента, а не `inherited` — иначе `ravo` выпадал из `<em>`.

Тесты: `first_letter_keeps_enclosing_inline_style`,
`first_line_does_not_clobber_inner_bold` (`box_tree::tests`). Оба падали до
фикса — второй именно на `FontWeight(400)` вместо `700`.

### Отпочковано

- [BUG-432](BUG-432-OPEN.md) — фон `::first-line` не красится вовсе
  (`BoxKind::InlineRun` не имеет собственной покраски бокса), хотя
  doc-комментарий `split_first_line_boxes` это обещает.

(исходное описание) ::first-letter drop-cap / ::first-line — TEST-58: CSS Pseudo-elements L4 §5.3-5.4
