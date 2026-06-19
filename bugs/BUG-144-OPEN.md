# BUG-144

**Статус:** OPEN (DEBTOR 4.36% — row-flip + gradient hard-stop + CPU backdrop-пайплайн исправлены; остаток = blur-качество)
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

CSS filter / backdrop-filter visual rendering (TEST-30).

## Исправлено 2026-06-17 — row-flip backdrop-filter (16.42% → 10.48%)

Карточки `backdrop-filter` (row 4) рисовались в неверном ряду:
элемент с bounds `y=439, h=102` в вьюпорте 718px появлялся на `y≈177`
(`718 − (439+102) = 177` — чистый вертикальный флип). Причина: `elem_id` —
GPU-FBO, в который рендерится содержимое элемента и который затем сэмплируется
как `Paint::image` в `composite_backdrop_filter_layer`. Он создавался с одним
лишь `PREMULTIPLIED`, без `FLIP_Y`, поэтому bottom-up строки FBO сэмплировались
вверх ногами (как opacity/filter offscreen-слои до BUG-133/BUG-146). Фикс:
`elem_id` создаётся через `offscreen_layer_image_flags()` (`PREMULTIPLIED |
FLIP_Y`). `filtered_backdrop_id` остаётся без флага — это CPU-upload (top-down).
В Lumen `backdrop-filter` всегда внутри offscreen-слоя (требование `from_level
>= 2`), так что `prev_render_target` — всегда FBO, и флип нужен всегда.

Тест: `offscreen_layer_flags_flip_y_and_premultiplied` (расширен doc).
TEST-30 → KNOWN_DEBTORS (`BUG-144`, 10.5).

## Gradient hard-stop (row 2) — исправлено 2026-06-19 (BUG-085, 10.48% → 7.56%)

`linear-gradient(to right, #e53e3e 50%, #38a169 50%)` рисовал только красную
половину — femtovg не дозаполнял хвост за последним стопом. Фикс в
`femtovg_stops` (см. BUG-085): последний цвет продлевается до 1.0.

## Backdrop colour-matrix/combo тёмные → CPU-пайплайн — исправлено 2026-06-20 (7.56% → 4.36%)

Карточки `backdrop-filter` с colour-matrix-фильтром (`grayscale`/`brightness`/
`invert`) и комбо (`blur(4px) brightness(0.6)`) рисовались тёмно-синими вместо
отфильтрованного градиента. Корень: `apply_backdrop_filters` загружал первый
`screenshot()` в текстуру, переключал render target на неё и **снимал
`screenshot()` повторно** — но FBO, подложенный `create_image`-загрузкой (равно
как и `filter_image`-назначением), читается обратно пустым, поэтому colour-matrix
применялся к чёрным пикселям. Фикс: весь chain backdrop-фильтров считается на CPU
по первому скриншоту — blur через `box_blur_rgba` (3-pass box ≈ Gaussian),
colour-matrix через общий `apply_filter_rgba`, один upload результата. Мёртвый
GPU-round-trip удалён целиком. Раньше работали только `bd-blur` (чистый blur,
card 1) и `bd-none` (контроль, card 6); теперь корректны все 6 карт.

## Остаток (DEBTOR, 4.36%)

1. **Blur-карты (row 4, cards 1 `blur(8px)` + 5 `blur(4px) brightness`):**
   `box_blur_rgba` — 3-pass box approximation, не точный Gaussian Edge; плюс
   edge-bleed (blur у верхней кромки карты затягивает тёмный `__f`-фон над
   градиентом bd-scene). Истинный backdrop-filter blur требует clamp в
   filter-region элемента.
2. **Filter pixel-parity (rows 1-3):** мелкие 1px AA-кромки grayscale/sepia/
   brightness/invert/contrast/saturate/hue-rotate vs Edge + gdigrab-шум.
