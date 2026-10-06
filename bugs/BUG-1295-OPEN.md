# BUG-1295 — SVG как `background-image`: внутренний размер и пропорции считаются неверно (корень в `%`, без `width`/`height`, `viewBox` с нулевой стороной)

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout/shell (`crates/shell/src/subresources.rs` — растеризация SVG-картинки, `crates/engine/image/src/svg.rs`; `background-size: auto|contain|cover` над SVG)


## Симптом

`--dump-layout` печатает `Загружена картинка: … (W×H)` — размер, в котором растеризован SVG. Фон `<div style="width:300px;height:300px;background:url(x.svg) no-repeat">`, корень SVG и результат (файлы `background-size/vector/support/*.svg`):

| корень SVG | растеризовано | ожидается (CSS Images 3 §5.3, SVG 1.1 §7.12) |
|---|---|---|
| `width="25%" height="50%"` | 25×50 (процент прочитан как px) | нет внутренних размеров и пропорций → размер из `background-size`/области |
| `width="50%"` | 50×100 | то же |
| `height="50%"` | 100×50 | то же |
| без `width`/`height` и без `viewBox` | 300×150 (размер по умолчанию replaced-элемента) | нет размеров, нет пропорций |
| `height="32px"` | 100×32 | ширина из пропорций (`viewBox`), а не константа 100 |
| `width="8px"` | 8×100 | высота из пропорций |
| `width="8px" viewBox="0 0 4 64"` | 8×64 | 8×128 (пропорция 4:64 → ×16 по высоте) |
| `viewBox="0 0 8 0"` (нулевая высота) | 100×100 | пропорции нет, внутреннего размера нет |
| `width="8px" viewBox="0 0 0 8"` (нулевая ширина) | 8×100 | пропорции нет |
| `width="8px" height="32px"` | 8×32 | верно |

Внутренние размеры/пропорции картинки считаются неверно, `background-size: auto|contain|cover|<length> auto` потом режет от них.

## Как найдено

WPT-RUN-14 срез 6: `css/css-backgrounds/background-size/vector/*` — 206 reftest, 171 FAIL, 35 PASS. По типу корня упавших SVG (`.tmp/wpt-run14/backgrounds-vector-classes.json`): 87 — `width`/`height` в `%`, 50 — один из двух опущен, 24 — оба опущены, 12 — абсолютные `width`+`height` (там причина другая — [BUG-1296](BUG-1296-OPEN.md)). Из них с разницей > 8 000 пикселей (то есть геометрия, а не шов): 69 id.

## Что делать

В загрузчике SVG-картинки (`crates/shell/src/subresources.rs`, `crates/engine/image/src/svg.rs`) вычислять внутренние ширину/высоту/пропорцию по CSS Images 3 §5.3: процент и отсутствие значения — «нет внутреннего размера»; пропорция — из `width/height` либо `viewBox` (обе стороны > 0), иначе «нет пропорций»; передавать в слой фона тройку (w?, h?, ratio?), чтобы `contain`/`cover`/`auto`/`<length> auto` считались по §3.9 (default sizing algorithm).

## Как проверить

`css/css-backgrounds/background-size/vector/{tall,wide}--*.html` (≈ 150), `zero-*-ratio-*.html`, `diagonal-percentage-vector-background.html`, `background-size-vector-0{01…29}.html`; `background-size/background-size-{contain,cover}-svg-view.html`.
