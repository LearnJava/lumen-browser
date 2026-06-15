# BUG-055

**Статус:** FIXED 2026-06-04
**Компонент:** layout
**Файл:** `crates/engine/layout/src/lib.rs:12798`

## Описание

tests::collect_picture_unsupported_type_falls_back: AVIF теперь поддерживается (supported_mime_types_includes_avif), поэтому fallback не нужен. Тест переписан в BUG-046 (2026-05-30): unsupported_type_falls_back переведён на image/heic (реально неподдерживаемый) и проходит.
