# BUG-1750 — фон inline-элемента не рисуется под его вложенными inline-элементами

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-VTT-SCOUT)
**Область:** layout/paint (фон inline-бокса строится по собственным текстовым фрагментам, а не по всему содержимому inline-бокса на строке)

## Симптом

```html
<body style="margin:0;font:14px sans-serif"><span style="background:#fc0">a <b>bold</b> c</span></body>
```

`lumen --dump-display-list`:

```
FillRect (0.00, 1.00, 8.00, 16.00) #ffcc00ff      ← "a "
FillRect (42.00, 1.00, 7.00, 16.00) #ffcc00ff     ← " c"
DrawText (11.68, ...) "bold" ...                  ← под ним фона нет
```

Жёлтый фон есть под «a » и « c», под «bold» (x 8…42) — дыра. По CSS 2.2 §9.4.2 / CSS Inline фон inline-бокса покрывает всё его содержимое на строке, включая вложенные inline. Дефект в display list, значит виден и на GPU, и на CPU-пути. С `<span class="bg"><b>только b</b></span>` фона нет вовсе.

## Как найдено

WPT-VTT-SCOUT (`docs/wpt-vendor-notes/webvtt.md`): эталоны `webvtt/rendering/cues-with-video/processing-model/bidi/*-ref.html` (`.cue > span {background; color:white}` с `<bdo>` внутри) рендерятся пустыми — 7 ложных PASS (`bidi_ruby`, `u0041_first`, `u05D0_first`, `u0628_first`, `u06E9_no_strong_dir`, `vertical_lr`, `vertical_rl`). Ещё замечено, не локализовано: пробел после `</bdo>`/`<span dir>` визуально теряется (`bidi/bdo-ltrend`).

## Что делать

Фон (и рамку) inline-бокса рисовать по объединённому прямоугольнику его фрагмента на строке, включая вложенные inline-боксы и atomic inline, — один `FillRect` на фрагмент строки, а не по каждому собственному текстовому прогону.

## Как проверить

Пробник выше: один `FillRect` шириной ≈49 px. Регрессионный графический тест в `graphic_tests/` на `<span style=background>` с вложенными `<b>`/`<bdo>`.
