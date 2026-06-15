# BUG-047

**Статус:** FIXED 2026-05-30
**Компонент:** layout
**Файл:** `crates/driver/tests/test_48.rs`

## Описание

НЕ баг (мисдиагноз): line-clamp реально усекает контент — InlineRun внутри .box = 40/80/120/160 (1-4 строки). .box=160 у всех — корректный flex align-items:stretch, Edge рендерит так же (48-edge.png). Тест переписан на ground-truth, #[ignore] снят
