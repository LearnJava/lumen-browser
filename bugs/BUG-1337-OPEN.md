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

## Дополнение (P2, WPT-RUN-14 срез 12, `css/CSS2/normal-flow` + `margin-padding-clear`, 2026-10-07)

Тот же дефект в этих каталогах: 215 id `thin-only` с `<img>` в эталоне (`normal-flow/height-003.xht`, `block-formatting-context-height-001.xht`, `block-formatting-contexts-008.xht`) и ещё 21 `thick` с `<img>` в эталоне. На `width-036.xht` (1 cm) — `FillRect (8, 26, 38, 96)` у теста и `DrawImage (8, 25.72, 37.8, 96)` у эталона: дробные и ширина, и `y`. Закрытие обещает до 215 + 21 id только в этих двух каталогах (`docs/wpt-vendor-notes/css.md` §срез 12).

## Срез 13 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` tables/positioning/floats/floats-clear/abspos…)

Тот же дефект: 138 id `thin-only` с `<img>` в эталоне — `floats-clear/adjacent-floats-001.xht`, `clear-001.xht`, `clear-002.xht` (в основном `floats-clear`, `positioning/absolute-replaced-width-*`). Закрытие даёт до 138 id в этих каталогах; вместе со срезами 11–12 — `css/CSS2` целиком (`docs/wpt-vendor-notes/css.md` §срез 13).

## Срез 14 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` text/linebox/fonts/…)

65 id `thin-only` с `<img>` в эталоне (`bidi-text/bidi-box-model-010…`, `linebox/*`, 3 в `text`): `linebox` — 39, `bidi-text` — 23, `text` — 3. Закрытие даёт до 65 id; вместе со срезами 11–13 — `css/CSS2` целиком (`docs/wpt-vendor-notes/css.md` §срез 14).
