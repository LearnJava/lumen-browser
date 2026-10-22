# BUG-1398 — `contain` на `<html>`/`<body>` не прекращает перенос фона на холст

**Статус:** OPEN (часть про фон закрыта 2026-10-21, P6; остаток — `*-overflow-*`)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout/paint (перенос фона корня на canvas, `crates/engine/paint/src/display_list/` — root background propagation)

## Симптом

`html,body,p{margin:0;width:300px;height:200px} p{background:white} body{background:red} <X>{contain:<C>}`, `<p>x</p>`, `--screenshot --viewport 400x300`:

| `<X>` | `<C>` | пиксель (350,250) вне `p` |
|---|---|---|
| `body` или `html` | `none`, `layout`, `paint`, `size`, `style` | красный (255,0,0) во всех десяти случаях |

Тест `contain-body-bg-001` («layout containment on body prevents background propagation») ожидает отсутствие красного; названия `-002…004`, `contain-html-bg-001…004` — те же утверждения для `paint`, `size`, `style`.

`contain-body-overflow-001` и ещё шесть `*-overflow-*`: проба (`body{overflow:hidden;contain:layout}`, `div` 200×200 красный после `p`) красного не даёт ни при каком `contain`; снимок отличается от эталона на 120 px в `x=200…221, y=2…14` (край текста за `width:200px`). Причина не локализована, в кластер «фон» не входят.

## Как найдено

WPT-RUN-14 срез 17: 15 id `css-contain/contain-{body,html}-{bg,overflow}-*`, все `thick`.

## Что делать

Не переносить фон корня на холст, если у `<html>` или у `<body>` (источника фона) `contain` включает `layout`, `paint`, `size` или `style` (по названиям тестов). Сверить условие с CSS Containment 2 §«contain property» — для `size`/`style` в текущем черновике это не сказано явно, брать по тестам.

## Как проверить

`css/css-contain/contain-body-bg-001.html`, `contain-html-bg-004.html`.

## Срез 2026-10-21 (P6)

**Фон закрыт.** `canvas_background_color` (`crates/engine/layout/src/box_tree/entry.rs`) возвращает `None`, если `<html>` без собственного фона, а у `<html>` или `<body>` `contain` включает `size`/`layout`/`paint`/`style` (`strict`/`content` — тоже; `inline-size` — нет). Собственный фон `<html>` переносится как прежде. Тест `containment_on_root_or_body_blocks_bg_propagation`.

**Остаток `*-overflow-*` — не про `contain`.** Проба `p{width:200px}` + «Test passes if there is no red.» (без всякого `contain`): `--dump-layout` даёт одну строку шириной 200, растр рисует её ≈222 px (Inter шире оценщика раскладки, [docs/probe-method.md](../docs/probe-method.md) §Reftest-A/B). У Chrome абзац переносится. В тесте `body{overflow:hidden;contain:layout}` клипует по 200 px и обрезает «red.» → расхождение 120 px. Чинить надо расхождение замера текста раскладки и растра, а не `contain`.
