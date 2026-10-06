# BUG-1345 — У `<img>` отступ `padding` в `em`/`rem`/`%` растягивает картинку на область padding; в `px` — верно

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/box_tree/` — размер replaced-бокса при `padding` без `px`)

## Симптом

`<img width=10 height=20 style="padding-left:…">`, `DrawImage`:

| `padding-left` | прямоугольник | ожидается |
|---|---|---|
| `20px` | (20, y, **10**, 20) | верно |
| `2em` (`font-size:10px`) | (0, y, **30**, 20) | (20, y, 10, 20) |
| `2rem` | (0, y, **42**, 20) | (32, y, 10, 20) |
| `2%` | (0, y, **30.48**, 20) | (≈ 20, y, 10, 20) |

## Как найдено

WPT-RUN-14 срез 11: `borders/border-color-005.xht` (1 id; эталон `img + img { padding-left: 9.6em }`). Ещё 7 id с похожим эталоном отнесены к BUG-1338, потому что у них падает раньше float.

## Что делать

Резолвить `em`/`rem`/`%` padding replaced-элемента до построения `DrawImage`, как для `px`.

## Как проверить

`css/CSS2/borders/border-color-005.xht`.
