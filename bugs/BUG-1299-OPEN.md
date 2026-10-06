# BUG-1299 — `background-repeat` с двумя значениями (`repeat no-repeat`, `space no-repeat`, `round repeat`…) и `space`/`round` на одной оси не разбираются: слой получает `repeat`

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Тип:** нереализованная функциональность — двухзначный синтаксис `<repeat-style>` (CSS Backgrounds 3 §3.4) не разбирается; свойство относится к P4 (`CSS-SPECS.md`, строка `background-repeat`).
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout (`crates/engine/layout/src/style/values/background.rs::BackgroundRepeat` — один enum на обе оси; парсер принимает только одно ключевое слово)


## Симптом

`--dump-display-list`, `background-repeat: V` на `<div>` с `url()`:

| V | `repeat=` в `DrawBackgroundImage` | ожидается |
|---|---|---|
| `repeat-x` | `RepeatX` | верно |
| `space` / `round` | `Space` / `Round` | верно |
| `repeat no-repeat` | `Repeat` | = `repeat-x` |
| `no-repeat repeat` | `Repeat` | = `repeat-y` |
| `space no-repeat` | `Repeat` | `space` по x, `no-repeat` по y |
| `round repeat`, `round no-repeat`, `repeat space`, `space space` | `Repeat` | поосевое значение |

`enum BackgroundRepeat { Repeat, NoRepeat, RepeatX, RepeatY, Round, Space }` (`background.rs:215`) не умеет разные значения по осям: `RepeatX/RepeatY` — единственные «двухосевые» формы. Двухзначный ввод молча отбрасывается и слой остаётся `repeat`.

## Как найдено

WPT-RUN-14 срез 6: `background-repeat-space-*.html`, `background-repeat-round-*.html`, `background-repeat/*.xht` — 23 reftest, 14 из них с двухзначным значением в исходнике (остальные — округление `round`, пересекается с [BUG-1303](BUG-1303-OPEN.md)). Тест 147 (`space`) зелёный; `parsing/background-repeat-*` — в [BUG-1297](BUG-1297-OPEN.md); `round` с `origin` ≠ `border-box` — [BUG-1303](BUG-1303-OPEN.md).

## Что делать

Хранить `(RepeatStyle, RepeatStyle)` по осям, `repeat-x`/`repeat-y` — сахар; передать пару в `DrawBackgroundImage` и `bg_tile_geometry` (обе оси уже считаются раздельно).

## Как проверить

`css/css-backgrounds/background-repeat-space-{3,4,5,6,7,1c}.html`, `background-repeat-round-{1a,1b,1d,1e,2,3,4}.html`, `background-repeat/gradient-repeat-spaced-with-borders.html`.
