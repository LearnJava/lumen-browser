# BUG-875 — `window.visualViewport` отсутствует целиком

**Статус:** FIXED 2026-09-25 — дрейф трекера: закрыт GAP-VVPORT; offsetLeft/offsetTop = 0 — нет pinch-zoom, архитектурная граница; строка BUGS.md не была перенесена при закрытии задачи (сверка с кодом 2026-09-25)
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-VVPORT` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `visual-viewport`)
**Область:** `grep -rn visualViewport crates/` даёт ноль совпадений — ни в `crates/js/src/dom.rs`, ни в шелле
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`typeof window.visualViewport === "undefined"`, `'visualViewport' in window`
— `false`. Интерфейса `VisualViewport` (CSSOM View / Visual Viewport API:
`width`/`height`/`scale`/`offsetLeft`/`offsetTop`/`pageLeft`/`pageTop` и
события `resize`/`scroll`) нет ни в каком виде.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant visual-viewport`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
vv-present=undefined in-window=false
vv-throws TypeError: Cannot read properties of undefined (reading 'addEventListener')
vv-grown
```

Страница остаётся живой (тики идут), то есть это не зависание движка, а
брошенное на первой строке исключение.

## Цена по WPT

`visual-viewport/viewport-no-resize-event-on-overflow-recalc.html` — весь
файл: `runTest()` начинается с
`window.visualViewport.addEventListener('resize', …)`. Вся категория
`visual-viewport/` (не вендорена) стоит на этом же объекте. В остатке
WPT-RUN-5 файл числится TIMEOUT, а не FAIL, потому что исключение брошено
из колбэка `requestAnimationFrame` в момент снимка — до починки
[BUG-591](BUG-591-FIXED.md) от 2026-08-22 такое исключение никто не слышал.

## Что дальше

Phase 3 по объёму (нужен настоящий visual viewport с масштабом), но
дешёвая часть — сам объект с `width`/`height`/`scale = 1`/`offset* = 0`,
привязанный к layout viewport'у: она уже переводит тесты из TIMEOUT в
осмысленный FAIL/PASS.

**Обновление 2026-09-22 (GAP-VVPORT срез 1, P6):** дешёвая часть (сам
объект) была закрыта раньше, попутно фиксом BUG-481 — см. `ROADMAP.md`'s
`GAP-VVPORT` строку для полной хронологии. Этот срез закрыл вторую по цене
часть: реальную доставку `resize`/`scroll` на `window.visualViewport`
(раньше `onresize`/`onscroll` были объявлены, но не диспатчились ни разу) и
`resize` на верхнеуровневом `window` вообще (раньше слался только дочернему
`<iframe>`). Остаток — тот же, что был изначально: `scale`/`offset*` не
отражают реальный pinch-zoom, потому что модели pinch-zoom/`<meta viewport>`
scale-клэмпинга у движка нет.

**Обновление 2026-09-22 (GAP-VVPORT срез 2, P6):** `scale` подключён к
реальному пользовательскому зуму шелла (Ctrl+=/Ctrl+-/Ctrl+0) — новый
нативный `_lumen_get_zoom_factor` читает `zoom_factor: Arc<Mutex<f32>>` в
JS-рантайме, наполняемый `js.update_zoom_factor()` рядом с
`update_viewport_size()` в `relayout.rs`. Это честный page zoom, не
pinch-zoom и не `<meta viewport>` scale-клэмпинг — движок их не моделирует.
`offsetLeft`/`offsetTop`/`pageLeft` остаются `0`: layout- и visual-вьюпорт
всё ещё один и тот же прямоугольник, сдвигаться некуда без настоящего
различения между ними — та же Phase 3 задача, что и раньше.

**Обновление 2026-09-22 (GAP-VVPORT срез 3, P6, финал):** Phase 3 сделан —
`<meta viewport initial-scale>` больше не подмешивается в
`zoom::effective_viewport` (это раньше меняло реальный box-layout viewport,
`Lumen::relayout_viewport`), а идёт отдельным нативом
`_lumen_get_meta_viewport_scale` в `visualViewport.scale`/`width`/`height`.
Пользовательский Ctrl+=/Ctrl+-/Ctrl+0 (честный page zoom) реальную раскладку
по-прежнему меняет и на `visualViewport.scale` больше не отражается — иначе
был бы двойной счёт. Единственный оставшийся зазор — `offsetLeft`/
`offsetTop`/`pageLeft` (панорамирование pinch-zoom): у движка нет touch-ввода
вообще ни в какой форме, это структурная граница, не сужение задачи. Полная
хронология — `ROADMAP.md`'s `GAP-VVPORT`. Статус — `done`.
