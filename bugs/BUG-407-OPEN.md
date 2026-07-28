# BUG-407 — Canvas 2D: NaN-координата пути роняет весь процесс (`Option::unwrap()` в скан-лайн заливке)

**Статус:** OPEN
**Компонент:** paint (`crates/engine/canvas/src/rasterize.rs::fill_path`) — сортировка
пересечений скан-лайна с рёбрами пути
**Найден:** 2026-07-28 (P2), проба категории `WPT-VENDOR-html`
(`/html/canvas/element/path-objects/2d.path.quadraticCurveTo.nonfinite.html`)

## Симптом

Тест вызывает `ctx.quadraticCurveTo(NaN, ...)` / другие нечисловые (`NaN`/`Infinity`)
координаты у методов пути Canvas 2D, затем `fill()`. Спека
(HTML Living Standard, CanvasPath) требует в этом случае **тихо проигнорировать** вызов
(«If any of the arguments are infinite or NaN, then the method must return without adding the
new point») — вместо этого весь процесс `lumen.exe` падает:

```
thread 'lumen-v8' (21748) panicked at crates\engine\canvas\src\rasterize.rs:27:53:
called `Option::unwrap()` on a `None` value
thread 'lumen-v8' (21748) panicked at .../core/src/panicking.rs:225:5:
panic in a function that cannot unwind
```

Паника происходит в потоке рендер-движка (`lumen-v8`) внутри функции, помеченной
`extern "C"`/FFI-границей, где unwind запрещён — поэтому это не перехватываемая паника, а
немедленный abort всего процесса. Любая веб-страница с такой канвас-операцией (случайной или
намеренной) валит браузер целиком — это DoS, доступный со страницы без привилегий.

## Причина

```rust
// crates/engine/canvas/src/rasterize.rs:27
xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
```

`xs` — X-координаты пересечений рёбер пути с текущей скан-линией (`fill_path`, до этого —
`collect_lines`). Если координата вершины пути — `NaN` (что для f32 законно долетает сюда,
если JS-биндинг методов `CanvasPath` не валидирует аргументы на конечность до того, как они
попадают в `PathSegment`), то `t = (yf - y0) / (y1 - y0)` или сама вершина дают `NaN` в `xs`, и
`f32::partial_cmp` на паре, где один операнд `NaN`, возвращает `None` → `.unwrap()` паникует.

Корень — отсутствие валидации non-finite аргументов на входе в путь (там, где спека требует
её), а не в самом рисовальщике: рисовальщик получает то, что ему передал JS-шим. Судя по
структуре (`fill_path`/`collect_lines` — общий код для всех операций пути), это не специфично
для `quadraticCurveTo` — тот же класс краша ожидаем от `NaN`/`Infinity`, переданных в
`moveTo`/`lineTo`/`bezierCurveTo`/`arc`/`arcTo`/`rect`, если они не валидируются раньше.

## Как воспроизвести

```bash
target/dev-release/lumen.exe tests/wpt/html/canvas/element/path-objects/2d.path.quadraticCurveTo.nonfinite.html
```

Процесс падает вскоре после загрузки страницы (до отрисовки первого кадра теста).

## Что нужно сделать

1. Найти JS-биндинг методов `CanvasPath` (`moveTo`/`lineTo`/`quadraticCurveTo`/
   `bezierCurveTo`/`arcTo`/`arc`/`rect`/…) и добавить проверку `is_finite()` на все числовые
   аргументы **до** добавления сегмента в путь — по спеке метод должен вернуться без изменений
   (no-op), не бросать исключение и не паниковать.
2. Как defense-in-depth в `rasterize.rs` — заменить `.unwrap()` на устойчивую сортировку,
   переживающую `NaN` (`partial_cmp(...).unwrap_or(Ordering::Equal)` либо явный фильтр
   `xs.retain(|x| x.is_finite())` перед сортировкой), чтобы будущий похожий пробел в другом
   вызывающем коде не валил процесс целиком.
3. Проверить остальные операции `CanvasPath`/`Path2D` тем же тестовым паттерном
   (`*.nonfinite.html` в `tests/wpt/html/canvas/element/path-objects/`) — вероятно, там есть
   аналогичные тесты для других методов, стоит прогнать их все разом.

## Связанное

Найден при вендоринге и пробе `WPT-VENDOR-html` (`docs/wpt-status.md`); тот же класс, что
предыдущие находки `partial_cmp`/`unwrap` в геометрии — искать похожие точки грепом
`\.unwrap\(\)` рядом с `partial_cmp`/`sort` в `crates/engine/canvas/` и `crates/engine/paint/`.
