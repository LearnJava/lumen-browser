# BUG-277 — wgpu-бэкенд: 38/141 графических тестов не проходят против эталона Edge (установленный wgpu-базлайн)

**Статус:** OPEN — установленный базлайн; не регрессия, никогда не тестировалось
**Компонент:** paint (`WgpuBackend` / `renderer.rs`) — wgpu рендеринг отличается от femtovg
**Найден:** 2026-07-13, первый полный прогон `LUMEN_BACKEND=wgpu graphic_tests/run.py --continue-on-fail`
  (после фикса BUG-276, commit `p1-wgpu-bug276`)

## Контекст

Это **не регрессия**, а установленный факт: wgpu-путь никогда не тестировался против Edge-эталонов
до задачи `P1-wgpu-bug276`. Данный файл фиксирует базлайн для финального гейта Фазы 3
(`Ph-wgpu-default`): когда wgpu станет дефолтным бэкендом, все перечисленные тесты должны
пройти или получить обоснованный `KNOWN_DEBTOR`.

## Результаты прогона (2026-07-13, commit e8bd5bd0 + BUG-276 fix)

```
LUMEN_PROFILE=dev-release LUMEN_BACKEND=wgpu python graphic_tests/run.py --continue-on-fail
65 PASS · 38 FAIL · 38 DEBTOR (существующие femtovg-долги) · 0 ERR
```

TEST-00: PASS 0.00% (BUG-276 FIXED в этом же коммите)

### FAIL-тесты (wgpu-специфичные расхождения с Edge)

| TEST | diff% | Регион (LTRB примерно) |
|------|-------|------------------------|
| 14   | 1.63% | y:165-264 x:25-648     |
| 24   | 0.50% | y:172-227 x:20-107     |
| 26   | 11.24% | y:41-480 x:41-470     |
| 30   | 10.24% | y:31-540 x:31-998     |
| 31   | 3.99% | y:181-620 x:41-635    |
| 36   | 7.80% | y:25-594 x:25-774     |
| 39   | 12.66% | y:21-332 x:21-848    |
| 45   | 5.81% | y:0-423 full-width    |
| 49   | 28.15% | y:0-424 full-width   |
| 53   | 5.45% | y:23-515 x:33-770    |
| 54   | 2.32% | y:25-684 x:19-976    |
| 56   | 14.12% | y:50-518 x:60-821   |
| 59   | 23.65% | y:1-403 x:25-1022   |
| 60   | 0.74% | y:20-650 x:28-989    |
| 62   | 16.07% | y:0-710 full-width  |
| 63   | 5.46% | y:33-439 x:11-1023  |
| 65   | 5.45% | y:10-695 x:1-989    |
| 68   | 3.17% | y:21-232 x:21-920   |
| 72   | 1.29% | y:41-300 x:31-330   |
| 74   | 3.74% | y:25-220 x:25-998   |
| 76   | 20.15% | y:221-525 x:6-999  |
| 81   | 3.44% | y:260-459 x:152-871 |
| 83   | 11.91% | y:33-520 x:51-972  |
| 100  | 1.09% | y:25-674 x:365-1022 |
| 101  | 20.00% | y:25-674 x:25-1004 |
| 103  | 1.79% | y:35-652 x:46-983   |
| 104  | 19.94% | y:25-674 x:25-1004 |
| 107  | 7.27% | y:65-621 x:65-933   |
| 109  | 7.53% | y:58-682 x:55-1001  |
| 111  | 1.27% | y:25-284 x:25-196   |
| 112  | 7.18% | y:48-530 x:44-347   |
| 113  | 6.10% | y:41-240 x:41-958   |
| 116  | 2.40% | y:147-700 x:27-1016 |
| 130  | 1.00% | y:112-604 x:149-945 |
| 132  | 8.11% | y:68-706 x:47-678   |
| 138  | 0.38% | y:30-650 x:21-1006  |
| 140  | 2.17% | y:42-464 x:41-386   |
| 141  | 1.59% | y:74-418 x:141-612  |

## Причина (гипотеза)

Femtovg и wgpu рендерят субпиксели, AA-кромки и текст по-разному. Femtovg за годы работы
«подстроился» под Edge-эталоны через KNOWN_DEBTOR-цикл. Wgpu-путь этих итераций не проходил.

Крупные провалы (≥10%): TEST-49 (28%), TEST-59 (24%), TEST-76/101/104 (~20%), TEST-62 (16%),
TEST-56 (14%) — вероятно, системные различия (сглаживание текста, градиенты, blending-mode).

## Что нужно для закрытия (Фаза 3 / `P1-wgpu-flip`)

1. Запустить `LUMEN_BACKEND=wgpu python graphic_tests/run.py --continue-on-fail` на финальном
   флипнутом коде и убедиться что все 38 тестов либо проходят, либо имеют обоснованный
   `KNOWN_DEBTOR` (аналогично femtovg-базлайну).
2. Первоочерёдные кандидаты для анализа: большие провалы (≥10%) — там, вероятно, сломан
   конкретный render-путь, а не просто AA-дрейф.
3. После флипа дефолта: перегнать весь набор с пустым `LUMEN_BACKEND` — если результат совпадает
   с wgpu-базлайном здесь (за вычетом закрытых багов), базлайн валидирован.

## Срез 1 (2026-07-21, P1) — TEST-76 root-cause найден и исправлен

`renderer.rs`'s wgpu paths for `DrawLinearGradient`/`DrawRadialGradient`/`PushMaskLinearGradient`/
`PushMaskRadialGradient` resolved `Px`/`Calc` stop positions via
`resolve_gradient_stops(stops, 1.0)` — a hardcoded `line_len = 1.0` instead of the actual
gradient-line length in CSS px that `cpu_raster.rs`/femtovg use (`linear_uv_endpoints`'s
`2.0 * half_len`, and `radius_x.max(radius_y)` for radial). On TEST-76's
`calc(50% ± 2px)` hard-stop linear gradient this collapsed/mis-scaled the stop band, producing
the 20.15% divergence. Fixed: `linear_gradient_uv_endpoints` now returns `line_len` alongside the
UV endpoints (mirrors `cpu_raster::linear_uv_endpoints`); the radial draw path uses
`radius_x.max(radius_y)`; the two mask variants use the equivalent formulas from
`cpu_raster::render_mask`. TEST-76 74× improved: 20.15% → 0.96% (matches the pre-wgpu femtovg
baseline of 0.64%, residual is AA/font-parity — diff image confirms only thin path-edge fringes
remain, no band mispositioning). `KNOWN_DEBTORS['76']` ratcheted to 0.96%.

Does **not** close BUG-277: the fix only applies to gradients with `Px`/`Calc` stop positions.
TEST-101/104/59/etc remain at their prior numbers — unrelated causes (border-radius AA,
image-set blend, non-gradient tests). Next candidates for a further slice: TEST-49 (28.15%,
background-blend-mode), TEST-62/56 (scroll-snap/border-radius, ≥14%).

## Срез 2 (2026-07-22, P1) — TEST-49 background-blend-mode root-cause найден и исправлен

`background-blend-mode` на top-level боксе (без родительского stacking-context) не
композитился вовсе в wgpu-окне: фоновые blend-слои сидели на `from_level == 1`, чей
«родитель» — реальная swapchain-поверхность, у которой нет `TEXTURE_BINDING` usage и её
нельзя сэмплировать. `renderer.rs`'s `Composite` требует `from_level > 1`, чтобы прочитать
родительский слой, поэтому blend молча падал в plain alpha-over — эффект пропадал целиком
(отсюда 28.15% на TEST-49).

Три части фикса:
1. **Изоляция (`display_list.rs::emit_background_image`).** Когда не-нижний фоновый слой
   реально блендит (`blend_mode != Normal`; `i == 0` всегда подавляется), стек слоёв
   оборачивается в собственную `PushOpacity{alpha:1.0}`/`PopOpacity`-группу. Это даёт
   blend-паре свой двухуровневый offscreen-стек независимо от вложенности предков (нижний
   слой на уровне изоляции, верхний — на уровень выше), так что `Composite` идёт на
   `from_level == 2` (родитель = сэмплируемая offscreen-текстура). Совпадает с семантикой
   CSS Compositing L1 §8.3 «background forms an isolated group» и с cpu_raster/femtovg
   (их immediate-mode canvas на этот момент уже содержит только контент самого бокса —
   CPU-снимки не изменились, `cpu_snapshots_match_references` зелёный).
2. **Un-premultiply в `BLEND_SHADER`.** Offscreen-слои копят ПРЕМУЛЬТИПЛИРОВАННЫЙ контент
   (каждый draw композитится straight-alpha `ALPHA_BLENDING` от прозрачно-чёрного clear →
   rgb остаётся домноженным на alpha). Формулы CSS Compositing L1 §8 ждут straight Cs/Cd —
   теперь делим обратно на alpha (`select(rgb/a, 0, a<=0)`). Прежний код трактовал
   премультиплированный rgb как straight → неверно при alpha<1; заодно чинит «изолирующий
   offscreen-слой чернел при multiply-против-прозрачного».
3. **Per-composite uniform-буфер blend-режима.** `self.blend_mode_uniform` писался через
   `queue.write_buffer` один раз на `PushBlendMode`/`PopBlendMode`; при 2+ таких парах в
   кадре все blend-проходы читали ПОСЛЕДНЕ записанный режим (все write_buffer лендятся до
   сабмита единого энкодера) — тот же класс hazard'а, что уже решён для `filter_param_bufs`.
   Теперь — свежий буфер на каждый composite (`make_blend_mode_param_buf`).

Результат: **TEST-49 28.15% → 2.74%** (10×; остаток — AA-кромка/font-parity, rule 2/3),
KNOWN_DEBTOR ратчет. TEST-148 (isolation) 6.30% → 5.44% — un-premultiply убрал чернение
изолирующего слоя. Полный wgpu-прогон 1–71 без регрессий (дрейф в пределах gdigrab-допуска);
пути не-blend страниц не задеты (display-list байт-в-байт), подтверждено
`cpu_snapshots_match_references`.

Does **not** close BUG-277: mix-blend-mode на top-level боксах (TEST-56 14.12%, и остаток
TEST-148) — отдельный путь, не покрытый background-only изоляцией этого среза; требует
изоляции для самих mix-blend-элементов. Не-blend долги (border-radius AA, filter, image-set,
SVG-AA) не тронуты.

## Новые долги, добавленные после базлайна

- **TEST-148 (isolation, 5.44%)** — добавлен 2026-07-18 (`p4-isolation`). `mix-blend-mode`-зависимый
  тест `isolation: isolate`: в wgpu-окне неизолированный `mix-blend` не композитится (source-over
  вместо `multiply`), а изолирующий offscreen-слой делает `multiply`-против-прозрачного-фона
  чёрным. Фича сама корректна — CPU-снимок (`lumen --screenshot`, `cpu_raster`) пиксельно совпадает
  с Edge, unit-тесты зелёные. Тот же класс, что TEST-56. Срез 2 (2026-07-22) убрал чернение
  изолирующего слоя (un-premultiply) — 6.30% → 5.44%; остаток = неизолированный top-level
  mix-blend, уйдёт в PASS с mix-blend-срезом BUG-277.
