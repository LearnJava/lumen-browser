# BUG-231

**Статус:** FIXED 2026-06-23 (D2-2, ветка `p1-bug231-anim-color`)
**Компонент:** paint/shell
**Файл:** `crates/engine/paint/src/display_list.rs` (`emit_box_self`, `apply_color_override`, `walk_with_anim`), `crates/engine/layout/src/animation.rs` (`CompositorOverride`, `to_compositor_frame`), `crates/shell/src/main.rs` (compositor offload, ~10271)

## Описание

Анимированные `background-color` / `color` НЕ применялись в живом окне без relayout.

`AnimationScheduler`/`TransitionScheduler` корректно вычисляли override цвета фона
(`AnimatedStyle.background_color`), но compositor-offload путь
(`AnimationFrame::to_compositor_frame` → `CompositorOverride`) переносил на дисплей-лист
только `opacity` и `transform`. Цветовые свойства помечались «требуют relayout» и
оседали в `anim_frame` без применения.

Следствие: любая `@keyframes` / `transition`-анимация цвета фона визуально не
менялась в окне (femtovg) — бокс держал базовый цвет. Видно на TEST-78: subject
`view-timeline` (`animation-timeline: view(block)`) ехал вправо по view-прогрессу
(transform композится), но его фон оставался базовым teal `#00838f` вместо
проинтерполированного `#e65100 → #ff8f00` (оранжевый у Edge).

## Как починено

1. В `CompositorOverride` (`lumen-layout/src/animation.rs`) добавлены поля
   `color: Option<Color>` и `background_color: Option<Color>`; `to_compositor_frame`
   теперь включает узлы, у которых задан любой из четырёх offload-свойств
   (opacity / transform / color / background-color).
2. `emit_box_self` (живой ordered-путь через `fill_buckets`) запоминает индекс
   начала своих команд (`cmd_start`) и в конце вызывает `apply_color_override`:
   - background-`FillRect`/`FillRoundedRect` патчится по совпадению `rect` с
     `background_clip_rect` бокса (drop-shadow заливки используют другой,
     смещённый/раздутый rect → не задеваются);
   - border-цвета и outline перерезолвятся из стиля бокса против overridden
     currentColor.
3. `walk_with_anim` Block-ветка (нон-ordered путь `build_display_list_with_anim`)
   применяет override инлайн (фон + border currentColor).

Ограничение: патчатся только уже присутствующие заливки — переход, стартующий с
полностью прозрачного фона, по-прежнему требует relayout для инъекции `FillRect`
(редкий случай; типичный transition интерполирует между двумя непрозрачными цветами).

## Результат

TEST-78 10.07% → **9.54%** (view-бокс показывает интерполированный оранжевый фон).
Остаток теста — font-parity текста (заголовок + метки + `.info` моноблок, Inter vs
Edge UI-шрифт, класс BUG-128) — доминирует и не закрывается этой задачей; TEST-78
остаётся KNOWN_DEBTOR под BUG-127 до сквозной задачи font-parity (FP-1/BUG-128).

Регресс-тесты: `anim_background_color_override_patches_fill_ordered` (paint),
`compositor_frame_carries_color_overrides` + `compositor_frame_skips_height_only_override`
(layout).
