# BUG-044

**Статус:** FIXED 2026-05-29
**Компонент:** shell
**Файл:** `shell/src/main.rs:4219, 4265`

## Описание

lumen-shell не компилируется (default + --features quickjs): non-exhaustive match по DisplayCommand в content_height_of/content_width_of — новые варианты PushMaskLayer/PopMaskLayer/DrawSvgPath/BoxModelOverlay (P2-мерджи) не обработаны; PushMaskLayer несёт rect → в rect-ветку, остальные → continue
