# BUG-1230 — отсоединённый `new Image()` во фрейме не получает `load`/`error`

**Статус:** OPEN
**Заведён:** 2026-10-01 (P3, при закрытии [BUG-1148](BUG-1148-FIXED.md)).
**Область:** shell (`crates/shell/src/frames.rs`, `crates/shell/src/dynamic_image_hook.rs`).

## Симптом

Хук немедленной загрузки фрейма пишет `script_nodes` в собственный `ImageRequestLedger`, а
`apply_stream_intrinsic_sizes` читает только реестр и дерево страницы. Картинка во фрейме запрашивается
сразу, пиксели доезжают в рендерер, но `load`/`error` и `complete` у `<img>`, не вставленного в документ
фрейма, не приходят.

## Что сделать

Доставлять события по `ImageDecoded`/`ImageDecodeFailed` в рантайм фрейма (по ключу src + id фрейма).
