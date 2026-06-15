# BUG-069

**Статус:** FIXED 2026-06-08
**Компонент:** image
**Файл:** `crates/engine/image/src/lib.rs:31`

## Описание

collect_picture_unsupported_type_falls_back падал: heic-source не скипался. Корень — D-3/D-4 добавили image/jxl, image/heic, image/heif в supported_mime_types(), хотя decode_jxl/decode_heic — заглушки (всегда Err). Picker выбирал heic-source и показывал пустую коробку вместо fallback. Fix: убраны 3 stub-формата из списка (avif остаётся — реальный декодер за feature-флагом)
