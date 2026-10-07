# BUG-1398 — `contain` на `<html>`/`<body>` не прекращает перенос фона на холст

**Статус:** OPEN
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
