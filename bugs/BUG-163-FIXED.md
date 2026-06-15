# BUG-163

**Статус:** FIXED 2026-06-15
**Компонент:** shell / paint
**Файлы:** `crates/engine/paint/src/display_list.rs` (`LazyImageSlot`),
`crates/engine/paint/src/backends/femtovg_backend.rs`,
`crates/engine/paint/src/renderer.rs`,
`crates/shell/src/main.rs` (`apply_loaded_page`)

## Описание

Картинки на lenta.ru не отображались (вместо них — серые прямоугольники).

Первоначальная гипотеза (preload-хинты / контент строит JS) оказалась неверной:
статический HTML lenta.ru **содержит** 116 `<img>`-тегов, все с
`loading="lazy"`. Реальных причин было две.

### Причина 1 — lazy-картинка никогда не рисовалась

Display-list эмитит для `<img loading="lazy">` команду `LazyImageSlot`, а не
`DrawImage`. Все бэкенды (`femtovg_backend`, wgpu `renderer`) рисовали по этой
команде **всегда серый placeholder** — даже после того, как shell скачивал и
регистрировал картинку через `Renderer::register_image`. Атрибут
`loading="lazy"` со временем не сбрасывается, поэтому при relayout снова
эмитился `LazyImageSlot` → серый. Итог: lazy-картинки не показывались никогда.

### Причина 2 — above-the-fold lazy-картинки не дозагружались на initial paint

Proximity-check (`deliver_layout_observers` → IntersectionObserver →
`take_lazy_image_requests` → fetch) вызывался **только** в `relayout()`, который
на первичной загрузке не выполняется (только при scroll/resize/zoom). Значит,
видимые сразу lazy-картинки не загружались до первого скролла.

## Воспроизведение

```
./target/release/lumen.exe https://lenta.ru
```

Раньше: лид-картинка статьи — серый прямоугольник.
Теперь: картинка отображается на первом же кадре.

## Исправление

1. **`LazyImageSlot` теперь несёт `object_fit`/`object_position`** и бэкенды
   рисуют по нему зарегистрированную картинку (fallback на серый placeholder,
   если ещё не загружена) — идентично `DrawImage`. `femtovg_backend`
   переиспользует `draw_image_in_rect`; wgpu `renderer` — ту же логику, что и
   `DrawImage`, плюс pre-pass `ensure_image_gpu_key`.
2. **`apply_loaded_page`** после `register_lazy_images` сразу прогоняет
   proximity-check: пушит layout-rects + viewport в JS, дёргает
   `deliver_layout_observers`, дренит `take_lazy_image_requests` и фетчит
   above-the-fold картинки, после чего запрашивает повторный redraw.

## Регрессионный тест

`lumen-paint` → `display_list::tests::lazy_img_slot_carries_object_fit`.
