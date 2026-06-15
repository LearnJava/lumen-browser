# BUG-037

**Статус:** FIXED 2026-05-26
**Компонент:** paint
**Файл:** `crates/engine/paint/src/renderer.rs`

## Описание

CSS filter effects не применяются визуально (grayscale/sepia/blur/etc.) — shared filter_uniform перезаписывался; fix: per-pass буфер через mapped_at_creation

## Детали

CSS-фильтры `grayscale`, `sepia`, `brightness`, `invert`, `contrast`, `saturate`, `hue-rotate`, `blur` и `backdrop-filter` присутствуют в дисплей-листе с правильной структурой (`PushFilter [grayscale]` / `FillRect` / `PopFilter`). Шейдер WGSL (`FILTER_SHADER_SRC`) корректно реализует все виды фильтров. Но визуально элементы отображаются без фильтрации — как если бы `PushFilter`/`PopFilter` игнорировались.

**Что работает:**
- Дисплей-лист: `PushFilter`/`PopFilter` генерируются корректно с правильными `FilterFn`
- `filter_fn_to_entry`: корректно маппит Grayscale→kind=3, Sepia→kind=8 и т.д.
- WGSL shader: логика `apply_filter_fn` математически верна

**Что не работает:**
- Итоговый рендер: все элементы с фильтром показывают исходный цвет без изменений
- backdrop-filter: полупрозрачные боксы с backdrop-filter рендерятся как пустые

**Где смотреть:**
- `crates/engine/paint/src/renderer.rs:4653` — `RenderPlanItem::FilterComposite` (исполнение)
- `crates/engine/paint/src/renderer.rs:3994` — `DisplayCommand::PushFilter` (планирование)
- Подозрение: offscreen texture не получает draw-команды, или FilterComposite читает неправильный слой
