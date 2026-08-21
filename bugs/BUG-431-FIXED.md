# BUG-431

**Статус:** FIXED 2026-08-21
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

Содержимое замещённых элементов `<img>`, `<video>` (кадр GIF / poster) и
`<iframe>`-заглушки красится в **border-box**, а не в content-box: обе ветки
покраски (`emit_box_self` и `walk`) кладут `DisplayCommand::DrawImage { rect:
b.rect, … }`, где `b.rect` — border-box. Картинку из-за этого растягивает на
рамку и padding и сдвигает под рамку на её ширину.

CSS Box L3 §1 / CSS Images L3 §5: object-fit/object-position работают
относительно **content-box** замещённого элемента.

## Воспроизведение

`.tmp/img-border.html`:

```html
<style>*{margin:0;padding:0} img{border:10px solid red;padding:5px}</style>
<img src="nonexistent.png" width="100" height="80">
```

```
$ lumen.exe --dump-display-list .tmp/img-border.html
DrawBorder (0.00, 0.00, 130.00, 110.00) w=[10.00,10.00,10.00,10.00] …
DrawImage  (0.00, 0.00, 130.00, 110.00) src="nonexistent.png" alt=""
```

Ожидается `DrawImage (15.00, 15.00, 100.00, 80.00)` — border-box 130×110 минус
рамка 10px и padding 5px с каждой стороны.

## Происхождение

Найден 2026-07-29 при разборе [BUG-099](BUG-099-OPEN.md): у `<canvas>` был ровно
тот же дефект, и он там починен (хелпер `content_box_rect()` в
`display_list.rs`, обе ветки `BoxKind::Canvas`). Ветки `BoxKind::Image`,
`BoxKind::Video`, `BoxKind::Iframe` и `DisplayCommand::LazyImageSlot` остались
на `b.rect` — фикс не расширяли, чтобы не смешивать элементы в одном баг-фиксе.

## Фикс

`rect: b.rect` заменён на `rect: content_box_rect(b)` в ветках `BoxKind::Image`
(и `DisplayCommand::LazyImageSlot`, чей rect повторно используется как rect
уже загруженной картинки — BUG-163), `BoxKind::Video` (оба `DrawImage`: GIF-кадр
и poster) и `BoxKind::Iframe`, в обеих функциях покраски (`emit_box_self` —
упорядоченный stacking-context путь, и `walk` — обычный). Путь inline-заменяемой
картинки внутри текстового рана (`emit_text_frags`, `frag.img_src`) не тронут —
у него отдельный расчёт rect из геометрии `InlineFrag`, вне области этого бага.

4 новых юнит-теста (`img_bitmap_is_painted_into_the_content_box`,
`lazy_img_slot_is_content_box`, `video_poster_is_painted_into_the_content_box`,
`iframe_placeholder_is_painted_into_the_content_box`), по образцу BUG-099.

Полный `graphic_tests/run.py` подтвердил ожидание: TEST-18/19 сдвинулись в
пределах допуска known-debtor (BUG-219, ±2%), TEST-70 не сдвинулся (его
`<img>` без своей рамки/padding). Заодно найдены и снят с `KNOWN_DEBTORS`
TEST-30/TEST-92 (просели ниже 0.5% порога независимо от этого бага — их
страницы не содержат `<img>`/`<video>`/`<iframe>`, чистый шум измерения) и
обнаружен несвязанный дрейф TEST-71/BUG-199 (не в этом фиксе, воспроизведён и
на немодифицированном `main` — см. правки `graphic_tests/run.py::KNOWN_DEBTORS`
для деталей). Детерминированные CPU-снапшоты (`snapshot_cpu`) не изменились —
ни одна страница из `PAGES` не сочетает рамку/padding с `<img>`/`<video>`/
`<iframe>` напрямую на самом элементе.
