# BUG-191

**Статус:** FIXED 2026-06-20
**Компонент:** paint
**Тест:** TEST-52 (5.83% → 4.25% DEBTOR, KNOWN_DEBTORS BUG-128)

## Описание

`text-shadow` blur — подозрение на дефект PushFilter Blur (sigma scaling,
multi-shadow stacking, offscreen layer sizing).

## Расследование

Blur-пайплайн text-shadow корректен — дефекта движка нет:

* **sigma = blur-radius / 2** (CSS Text Decoration L3 §6, как box-shadow и canvas
  `shadowBlur`). `emit_text_shadows` (`display_list.rs`) заворачивает `DrawText`
  тени в `PushFilter { Blur(blur/2) }` / `PopFilter` при blur > 0; при blur == 0
  тень рисуется напрямую.
* **Offscreen layer sizing.** Обе пиксельные реализации игнорируют `bounds` у
  `PushFilter` и аллоцируют offscreen-слой во весь RT (femtovg: GPU
  `ImageFilter::GaussianBlur` на full-RT FBO; cpu_raster: три-box приближение).
  Поэтому halo тени (~3σ вокруг глифа) НЕ клипуется к строчному боксу.
* **Multi-shadow stacking.** Несколько теней обходятся в обратном порядке
  (первая в CSS-списке рисуется поверх — §6), каждая blur-тень получает свой
  `PushFilter`/`PopFilter`.

Пиксельная проверка Edge-эталона vs Lumen (cropped cells):
- row 1 (одна красная тень, blur 0/4/10/20px): прогрессия мягкости halo совпадает
  с Edge; extent красного ореола вокруг глифа эквивалентен.
- row 2 cell 4 (glow-only `0 0 18px` синий по тёмному фону): белый «B» с синим
  ореолом — extent и интенсивность glow совпадают с Edge (чистейший тест blur'а,
  без зависимости от резкой составляющей глифа).

В diff-картинке доминируют **два несовмещённых начертания** глифов «A»/«B»
(cyan/white ghosts) — Edge рендерит serif 80px, Lumen — Inter sans; сами тени
near-black = совпадают. Остаток 4.25% целиком font-parity (rule 3).

## Решение

Production-кода не менялось (blur уже корректен). Добавлен регресс-тест
`text_shadow_blur_sigma_is_half_radius_for_test52_progression`
(`display_list.rs`), закрепляющий sigma = radius/2 для радиусов TEST-52
(0/4/10/20px → sigma 0(no-filter)/2/5/10). TEST-52 → `KNOWN_DEBTORS`
(`run.py`, BUG-128, baseline 4.25%).

## Воспроизведение

`LUMEN_PROFILE=dev-release python graphic_tests/run.py --only 52`
→ DEBTOR (4.25%, baseline 4.25%, BUG-128).
