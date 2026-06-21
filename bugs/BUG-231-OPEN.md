# BUG-231

**Статус:** OPEN
**Компонент:** paint/shell
**Файл:** `crates/engine/paint/src/display_list.rs` (`emit_box_self`, `walk_with_anim`), `crates/shell/src/main.rs` (`anim_frame` → compositor offload, ~10179)

## Описание

Анимированные `background-color` / `color` НЕ применяются в живом окне без relayout.

`AnimationScheduler`/`TransitionScheduler` корректно вычисляют override цвета фона
(`AnimatedStyle.background_color`), но compositor-offload путь
(`AnimationFrame::to_compositor_frame` → `CompositorOverride`) переносит на дисплей-лист
только `opacity` и `transform`. Цветовые свойства помечены «требуют relayout» и
оседают в `anim_frame` без применения (см. комментарий `main.rs:~10179`
«color/background-color остаются в anim_frame на будущее»).

Следствие: любая `@keyframes` / `transition`-анимация цвета фона визуально не
меняется в окне (femtovg) — бокс держит базовый цвет. Видно на TEST-78: subject
`view-timeline` (`animation-timeline: view(block)`) едет вправо по view-прогрессу
(transform композится), но его фон остаётся базовым teal `#00838f` вместо
проинтерполированного `#e65100 → #ff8f00` (оранжевый у Edge).

## Как починить

Сделать `background-color` (и `color`) compositor-offloadable, как opacity/transform:
1. Добавить `background_color: Option<Color>` в `CompositorOverride`
   (`lumen-layout/src/animation.rs`) и в `to_compositor_frame`.
2. Протянуть override в `emit_box_self` (ordered-путь, `fill_buckets`) и
   `walk_with_anim`: при наличии override патчить цвет background-`FillRect`
   (множество сайтов `FillRect { color: bg }` в `emit_box_self`).

Альтернатива (дороже): триггерить relayout/перестроение дисплей-листа при смене
цветового override.

## Контекст

Найдено при F2-2 (CSS Scroll-Driven Animations L1, BUG-127): scroll/view/named
timeline теперь драйвят прогресс анимации от скролла (transform/opacity видны),
но цвет фона subject-а не обновляется из-за этого общего ограничения композитора.
