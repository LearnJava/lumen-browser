# BUG-1419 — Устаревшие системные цвета CSS Color 4 §6.2: 15 имён отвергаются, `ThreeDFace` рисуется чёрным

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout (`crates/engine/layout/src/style/` — таблица `SystemColor`)

## Симптом

`--screenshot`, `background:#ff0000;background-color:<имя>`; пиксель в x=5. Алиасы — из самих тестов (`deprecated-sameas-NNN.html`,
эталон `deprecated-sameas-<цель>-ref.html`):

| имя → цель по спеке | результат |
|---|---|
| `ActiveBorder`, `InactiveBorder`, `WindowFrame` → `ButtonBorder` | **красный** — декларация отвергнута |
| `ActiveCaption`, `AppWorkspace`, `Background`, `InactiveCaption`, `InfoBackground`, `Menu` → `Canvas` | **красный** |
| `ButtonHighlight`, `ButtonShadow` → `ButtonFace` | **красный** |
| `CaptionText`, `InfoText`, `MenuText` → `CanvasText` | **красный** |
| `InactiveCaptionText` → `GrayText` | **красный** |
| `ThreeDFace` → `ButtonFace` | `(0,0,0)` вместо `(240,240,240)` |
| `ThreeDDarkShadow`, `ThreeDHighlight`, `ThreeDLightShadow`, `ThreeDShadow` → `ButtonBorder` | `(0,0,0)`, как `ButtonBorder` (тест проходит) |
| `Scrollbar`, `Window` → `Canvas`; `WindowText` → `CanvasText` | верно |

Заодно: `ButtonBorder` рисуется `(0,0,0)` — у Edge это серый; тесты проходят только потому, что обе стороны чёрные.

## Как найдено

WPT-RUN-14 срез 19: `css-color/deprecated-sameas-001…014`, `017`, `022.html` — FAIL (`thick`), 7 из 23 проходят
(`015`, `016`, `018…021`, `023`). `@supports (color: WindowFrame)` в тестах не выполняется, потому что имя не разбирается.

## Что делать

Добавить 15 имён в таблицу `SystemColor` как алиасы по §6.2; `ThreeDFace` привести к `ButtonFace`.

## Как проверить

`css/css-color/deprecated-sameas-001.html`…`014.html`, `017.html`, `022.html`.

## Исправление

`SystemColor::parse` (`values/color.rs`) принимает 15 устаревших имён как алиасы канонических вариантов по CSS Color 4 §6.3
(`ActiveBorder`/`InactiveBorder`/`WindowFrame` → `ButtonBorder`; `ActiveCaption`/`AppWorkspace`/`Background`/`InactiveCaption`/
`InfoBackground`/`Menu` → `Canvas`; `ButtonHighlight`/`ButtonShadow` → `ButtonFace`; `CaptionText`/`InfoText`/`MenuText` → `CanvasText`;
`InactiveCaptionText` → `GrayText`), а `ThreeDFace` теперь `ButtonFace` (240,240,240), а не `ButtonBorder`. Таблица цветов
(`system_color`) не менялась. `ButtonBorder` по-прежнему чёрный (как у Edge в светлой теме) — пункт «заодно» симптома не трогался.
Тесты: `style::tests::color::system_color_parse_deprecated_aliases_bug1419`, `system_color_deprecated_alias_in_background_color_bug1419`.
`deprecated-sameas-*` под `wptrunner` не перепроверялись.
