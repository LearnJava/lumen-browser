# BUG-1337 — `DrawImage` на дробной позиции не привязан к пикселям: у картинки 1-px полупрозрачная кромка, у блока рядом — нет

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs` — ветка `DrawImage`; тот же приём привязки, что для `FillRect`/`DrawBorder` в BUG-553 срез 32/38, не применён к картинкам)

## Симптом

Страница `<body style="margin:0"><p style="margin:0;height:25.72px">x</p><div style="background:blue;height:50px"></div><img src="blue.png" width="200" height="50" style="display:block">` (после первой — второй блок на 10 px ниже): строки `y = 26…75` у `div` чисто синие, у `<img>` на `y = 25` — `rgb(183,183,255)`, на `y = 75` — `rgb(71,71,255)` (смесь с белым). Те же размеры, та же дробная верхняя граница `25.72`.

## Как найдено

WPT-RUN-14 срез 11: 249 из 484 упавших reftest `css/CSS2/{backgrounds,borders}` отличаются от эталона **только** изолированными 1-px линиями (`thin-only`, `reftest_pixdiff.py --viewport 800x600 --ahem`); в 248 из них эталон содержит `<img … width="100%" height="N">` — «зелёный прямоугольник» эталона нарисован растянутой картинкой 1×1 (`support/1x1-green.png`), а в тестовом файле тот же прямоугольник — `div` с `background`. Пример `backgrounds/background-001.xht`: расхождение — строки `y = 25` и `y = 75`, 784 px каждая. Не дублирует BUG-1249: тот про заливки соседних боксов (закрыт привязкой `FillRect` в BUG-553), здесь — другая команда.

## Что делать

Привязать прямоугольник `DrawImage` к целым пикселям так же, как `FillRect` (CSS Painting: pixel snapping для replaced content). Править на уровне `DisplayCommand`, единообразно для CPU и wgpu (`crates/engine/paint/CLAUDE.md`). Эталонная картинка 1×1, растянутая на 800×50, — самый частый приём эталонов WPT, закрытие снимет около половины `thin-only` во всём `css`.

## Как проверить

`css/CSS2/backgrounds/background-001.xht` — `reftest_pixdiff.py` должен дать `identical`; затем `run_corpus.py --prefixes css/CSS2/backgrounds,css/CSS2/borders` — число `thin-only` (249) должно упасть.
