# BUG-144

**Статус:** OPEN (DEBTOR) — row-flip + gradient hard-stop + CPU backdrop-пайплайн + blur-качество (edge-bleed clamp + 3-pass Gaussian) исправлены; остаток = filter/edge AA + текст (rule 3), точечного P3-дефекта нет. KNOWN_DEBTOR baseline ратчет 4.36→4.27 (ревизия P3 2026-07-04)
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

## Blur-качество backdrop-карт — исправлено 2026-06-23 (4.36% → 4.27%)

Две проблемы blur-карт row 4 (cards 1 `blur(8px)`, 5 `blur(4px) brightness`):

1. **Edge-bleed (исправлено).** `apply_backdrop_filters` блюрил **весь**
   screenshot канвы и затем `composite_backdrop_filter_layer` вырезал bounds —
   поэтому box-blur у верхней кромки карты усреднял тёмный `__f`-фон, нарисованный
   *над* bd-сценой. По спеку (CSS Filter Effects §backdrop-filter) вход фильтра
   обрезается до border-box элемента. Фикс: `bounds` пробрасывается в
   `apply_backdrop_filters`, переводится в device-px и blur (`box_blur_rgba_region`)
   клампит окно сэмплирования к этому прямоугольнику — кромка дублирует краевые
   пиксели backdrop'а, а не тянет внешний фон.

2. **box-blur ≈ Gaussian (исправлено).** Прежний `box_blur_rgba` делал **один**
   2D box-проход (1 H + 1 V), несмотря на комментарий «3-pass» — крупный box
   читается «коробчато» против истинного Gaussian Edge. Заменён на честный
   3-итерационный box-blur (`gaussian_box_radii` по Kovesi *Fast Almost-Gaussian
   Filtering*, n=3; `box_blur_pass_region`). Регулярный `filter: blur()` и так
   использует GPU `ImageFilter::GaussianBlur`; теперь backdrop-blur ему близок.

## Остаток (DEBTOR, 4.27% — baseline KNOWN_DEBTORS 4.36%, в пределах ±2% шума)

**Filter pixel-parity (rows 1-3):** мелкие 1px AA-кромки grayscale/sepia/
brightness/invert/contrast/saturate/hue-rotate vs Edge + gdigrab-шум + текст
меток карт (rule 3). Метрика TEST-30 насыщена этими факторами, поэтому
blur-улучшения дают лишь ~0.1pp; визуально blur-карты теперь корректны.

## Ревизия P3 (2026-07-04)

- Перепроверка на свежей сборке main (после мержа BUG-127, `fa00ba34`):
  TEST-30 = **4.27%** (gdigrab, стабильно — совпадает с записанным в этом файле
  результатом от 2026-06-23). Baseline в `run.py` оставался 4.36% и не был
  затянут после blur-фикса — теперь ратчет **4.36 → 4.27**.
- **Diff-декомпозиция** (`screenshots/30-css-filter-diff.png`): весь остаток
  сконцентрирован по **кромкам**, залитых площадей нет:
  - rows 1–3 (`filter:` карты) — тонкие 1px вертикальные AA-полосы по левой/правой
    границе каждой карты: рассинхрон AA-ядра femtovg против downscale-ядра Edge
    (тот же класс, что BUG-176 border-radius edge-AA и BUG-247 SVG-stroke AA);
  - row 4 (`backdrop-filter` карты) — градиентная кайма по верхней/левой кромке
    blur-карт: blur-kernel (3-pass box ≈ Gaussian) против точного Gaussian Edge;
  - текст меток карт — font-parity (Inter vs Edge, rule 3, класс BUG-128).
- Filter-математика (`apply_filter_rgba`: grayscale/sepia/invert/…) спек-корректна
  и покрыта юнит-тестами (`apply_filter_rgba_*`), геометрия карт совпадает.
- **Точечного P3-дефекта нет.** Остаток инхерентен (rasterizer-vs-Edge AA + текст)
  и не закрывается P3-правкой — по прецеденту BUG-127/176/247 статус приведён к
  OPEN (DEBTOR), указатель перенесён в группу должников STATUS-P3.

KNOWN_DEBTORS['30'] = ('BUG-144', 4.27) — ратчет вниз 4.36 → 4.27.
