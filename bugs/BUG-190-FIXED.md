# BUG-190

**Статус:** FIXED 2026-06-23
**Компонент:** paint
**Тест:** TEST-49 (2.39% → 0.14% CPU snapshot)

## Описание

`background-blend-mode` (multiply/screen/overlay/darken/lighten/difference/
exclusion/color-dodge/luminosity) — TEST-49 расходился на 2.39%.

## Корневая причина (не blend-математика)

Per-box разбор показал: 9 из 11 боксов пиксель-в-пиксель совпадали с Edge,
а blend-формулы (`blend_modes.rs`) и tiny-skia дают идентичный канону результат
(проверено пробным тестом: color-dodge orange@0.8 над синим = (209,217,174) в
обоих). Расходились **только два бокса — единственные с градиентом, затухающим в
`transparent`** (color-dodge 65.5%, multi-cycling 4.7%).

Причина — **интерполяция стопов градиента в straight (не premultiplied)
пространстве**. CSS Images L4 §3.1 требует premultiplied. При straight-интерполяции
`rgba(255,200,100,0.8) → transparent` (`rgba(0,0,0,0)`) цвет тянется к чёрному, а
не сохраняет оранжевый оттенок при падении alpha → мутно-тёмная середина градиента;
color-dodge усиливал это в явный артефакт. Точный центр (чистый стоп) и края
(полностью прозрачно) совпадали — расходилась только зона затухания. Дефект общий
для обоих straight-интерполирующих бэкендов (tiny-skia CPU и femtovg-окно).

## Как починено

Новая общая функция `gradient_math::premultiplied_subdivide_stops` субдивизирует
каждый сегмент с разной alpha на концах в 16 промежуточных стопов, посчитанных
premultiplied-интерполяцией (`lerp_color_premul`: lerp premul-каналов → un-premult).
Straight-интерполяция между плотными стопами воспроизводит premultiplied-кривую.
Непрозрачные сегменты (равная alpha) отдаются без изменений → сплошные градиенты
байт-идентичны (нулевой регресс). Подключена в обоих стоп-билдерах:
`femtovg_stops` (окно) и `skia_gradient_stops` (CPU-снимок).

## Результат

TEST-49 CPU-снимок 2.20% → **0.14%** (PASS), color-dodge 65.5%→0.0%,
multi-cycling 4.7%→0.0%. Попутно TEST-45 (multiple-backgrounds, тоже rgba→transparent
слои) 8.95%→4.74% (остаток = BUG-115 percent background-size). Без регресса:
TEST-26 0.00%, TEST-39 1.40% (без изм.), TEST-40 0.05%, TEST-104 0.37%,
TEST-116 (gradient-interpolation) 0.38%.

## Воспроизведение

`python graphic_tests/run.py --only 49`
