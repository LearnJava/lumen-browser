# BUG-431

**Статус:** OPEN
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

## Ожидаемый фикс

Заменить `rect: b.rect` на `rect: content_box_rect(b)` в ветках `Image`
(включая `LazyImageSlot`), `Video` и `Iframe` в обеих функциях покраски. Скорее
всего сдвинет TEST-18/19/70 и часть страниц с рамками у картинок — потребуется
полный прогон `graphic_tests/run.py` и перегенерация CPU-снапшотов.
