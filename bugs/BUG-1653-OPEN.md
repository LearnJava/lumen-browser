# BUG-1653 — `background-attachment: local`: остаток после BG-ATTACH-LOCAL (ячейка таблицы, `rtl` scrollLeft, скруглённый `url()`-фон)

**Статус:** OPEN
**Заведён:** 2026-10-19 (P1, закрытие `BG-ATTACH-LOCAL`)
**Область:** paint (`display_list/background_mask.rs::local_bg_scroller`, `table.rs`), layout (`scroll_x` для `rtl`)

## Симптом

1. Ячейка таблицы с `overflow: hidden` и `background: url() local` (`table-cell-background-local-002/003`): `local_bg_scroller` пускает только `BoxKind::Block|FlowRoot`, ячейка красит фон отдельно в `table.rs` — слой не едет с содержимым.
2. `rtl`-скроллер с `scrollLeft` < 0 (`attachment-local-positioning-4`, `local-attachment-rtl-scroll`, `local-attachment-flex-row-reverse-scroll`): отрицательный `scroll_x` не доходит до слоя так, как ждёт WPT (смещение слоя расходится с ожидаемым).
3. Не про `local`, найдено попутно: `DrawBackgroundImage` не режется по `border-radius` (`emit_background_layer`, ветка `BackgroundImage::Url` — нет `PushClipRoundedRect`, в отличие от градиентов, BUG-631): `div { border-radius: 50%; background: url(x.png) }` рисует картинку по углам вне скругления. Из-за этого `attachment-local-clipping-image-4/5` расходятся с эталоном, который сам не скруглён.

## Как проверить

`css/css-backgrounds/background-attachment-local/attachment-local-positioning-4.html`, `attachment-local-clipping-image-4.html`, `table-cell-background-local-002.html`.
