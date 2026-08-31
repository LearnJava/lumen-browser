# BUG-931 — wgpu-рендер живого окна обрезает содержимое `PushScrollLayer` по высоте почти вдвое, хотя display list и computed-scissor верны

**Статус:** OPEN
**Компонент:** paint (`crates/engine/paint/src/renderer.rs` — wgpu `render_impl`,
`PushScrollLayer`/`FillRect`/`sync_scissor_to_stack`/`bind_and_draw!`/`run_draw_ops!`)
**Заведён:** 2026-08-31 (P3) при ревизии BUG-124 (TEST-51, первая строка
STATUS-P3.md) — отдельная находка, не относится к диагнозу BUG-124
(line-height/font-metrics)

## Симптом

TEST-51 (`graphic_tests/51-scrollbar-rendering.html`), третий scroll-контейнер
(«Horizontal scroll container», `overflow-x: scroll`, три `display:inline-block`
ребёнка `180×60` встык). Первый (`533483`, фиолетовый) ребёнок рисуется высотой
~31px вместо положенных 60px — визуально обрезан почти ровно пополам, а под
обрезкой видна не следующая фигура, а собственный фон контейнера
(`#16213e`), т.е. дочерний прямоугольник просто не дорисован, а не перекрыт.

Живой прогон (`graphic_tests/run.py --only 51`, gdigrab) — сэмплы по
x=300 в обеих версиях:

```
        Lumen        Edge
y=207   фон          фиолетовый   (Edge уже начал красить, Lumen ещё фон)
y=208   фиолетовый   фиолетовый
...
y=238   фиолетовый   фиолетовый
y=239   фон          фиолетовый   (Lumen обрывается здесь)
...
y=266   фон          фиолетовый   (Edge продолжает до сюда)
y=267   фон          фон          (совпало — оба закончили)
```

Lumen: фиолетовый 208→238 (≈31px). Edge: 207→266 (≈59px). Соседний бокс того
же теста («Vertical scroll container», `overflow-y: scroll`, дети `176×80`)
рисуется корректно на всю высоту — обрезка не универсальна для
`PushScrollLayer`, а специфична для этого контейнера.

## Изолировано на GPU/live-window путь

`--screenshot` (CPU-путь, `cpu_raster.rs`) и MCP `resource://screenshot`
рисуют этот же тест **без** обрезки — оба соответствуют display list
побитово (проверено на полной странице и на минимальном репро с
`<div style="overflow-x:scroll"><div style="display:inline-block;
width:180px;height:60px">…`). Обрезка воспроизводится только в реально
рисуемом окне (`--mcp-live-port`, gdigrab-скриншот) — т.е. в wgpu-пути
`renderer.rs`, а не в `cpu_raster.rs`/`femtovg_backend.rs` (последний вообще
не задействован в дефолтной сборке — `PushScrollLayer` там реализован, но не
вызывается: живой рендер идёт через `RenderPassMode::Normal` в
`renderer.rs`).

## Что уже исключено измерением

Временная инструментация (`eprintln!` в обработчике `DisplayCommand::FillRect`
перед `sync_scissor!()`) на живом окне (`LUMEN_DEBUG_BUG124=1`, без
`--screenshot`) показала для этого прямоугольника:

```
rect=Rect { x: 231.0, y: 208.2, width: 180.0, height: 60.0 }
xform = чистый перевод (0, +68)  // высота chrome-тулбара, dpr=1
clip_top (screen-space) = Rect { x: 227.0, y: 266.2, width: 296.0, height: 76.0 }
current_scissor (device px) = { x: 227, y: 266, width: 296, height: 77 }
surface = (1024, 792), cull_h = 792, mode = Normal
```

Прямоугольник после трансформа: экранный Y ∈ [276.2, 336.2]. Scissor:
Y ∈ [266, 343]. Прямоугольник **целиком внутри** scissor — по этим числам
обрезки быть не должно. То есть:

- геометрия FillRect в display list верна (`231, 208.2, 180, 60` — не
  затронута BUG-124: высота не 19.2-кратная, не связана с line-height);
- computed device-scissor в момент постановки команды в `sync_scissor!()`
  верен и не отсекает прямоугольник;
- `band_draw_fraction()` (рычаг среза BUG-405) не активен — `LUMEN_BAND_DRAW_FRACTION`
  не выставлена, `cull_h == surface_h`;
- `mode = Normal`, а не `Band` — это не срез scroll-blit полосы.

Значит расхождение — между ЭТИМ вычисленным `desired`-scissor и тем, что
реально применяется GPU в момент растеризации ЭТОГО draw-call'а: либо
`DrawOp::SetScissor` для этого прямоугольника не долетает до
`render_pass.set_scissor_rect()` в `run_draw_ops!` (склейка/элайдинг
`bind_and_draw!`/`flush_pending_draw!` при разборе показалась корректной, но
не прогнана под отладкой на этом самом кадре), либо сам вызов `draw()`
покрывает не те вершины, что предполагается (диапазон `v_start..v_end`).
Корень не найден — нужен следующий шаг инструментации ровно в
`run_draw_ops!` (напечатать фактический `s.x/s.y/s.width/s.height`,
переданный в `set_scissor_rect`, и диапазон вершин активного `draw()` для
этого прямоугольника), которого эта сессия сделать не успела.

## Почему не точечный P3-фикс

Код в `renderer.rs` вокруг `bind_and_draw!`/`run_draw_ops!`/`sync_scissor_to_stack`
— плотная, perf-критичная батчинг-логика (BUG-405, тот же файл), не
локальный CSS-баг. Нужна инструментация внутри самого цикла отправки
GPU-команд (`run_draw_ops!`), не просто на этапе построения `draw_ops`
(что уже сделано и не показало проблемы). Похожий класс дефекта уже был в
этом же файле (BUG-335 — `PushScrollLayer` не применял
`apply_transform_to_clip()`), но тот чинился на этапе ПОСТРОЕНИЯ clip, а
здесь построение уже проверено верным.

## Как повторить

1. `python graphic_tests/run.py --only 51` (живое окно, два чистых прогона
   для исключения gdigrab/фокус-ловушки CLAUDE.md) — диф в
   `graphic_tests/screenshots/51-scrollbar-rendering-diff.png`, видна
   сплошная не-1px область в третьем контейнере.
2. Для сравнения: `lumen.exe --screenshot out.png graphic_tests/51-scrollbar-rendering.html`
   — та же страница, CPU-путь, обрезки нет.
3. Минимальный репро (без остальной страницы теста, тот же результат в живом
   окне): `<div style="width:300px;height:80px;overflow-x:scroll;overflow-y:hidden;
   border:2px solid #0f3460"><div style="display:inline-block;width:180px;
   height:60px;background:#533483;vertical-align:top;margin:10px 4px 0 4px">
   </div>…</div>`.

## Связь с BUG-124

Обнаружен при ревизии BUG-124 (та же страница, TEST-51). BUG-124 — про
1px-сдвиг ряда 2 из-за flat-коэффициента line-height (домен P1,
PS-1/FP-1-класс). Этот баг — отдельный, про GPU-путь батчинга сцены, и не
объясняется line-height. `KNOWN_DEBTORS['51']` в `run.py` не поднимается
из-за него отдельно — вклад уже входит в текущий ратчет 1.67%.
