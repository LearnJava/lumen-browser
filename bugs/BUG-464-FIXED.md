# BUG-464: `document.elementFromPoint`/`elementsFromPoint` not implemented

**Статус:** FIXED 2026-09-01
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/v8_runtime/install/platform.rs`, шим
`crates/js/src/shim/web_api_shim_mid.js`) + paint (`crates/engine/paint/src/hit_test.rs`)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`

## Симптом

```
FAIL <name> - document.elementFromPoint is not a function
```

Стабильно повторяющийся `TypeError` в 10 файлах
(`floats/hit-test-floats-00{1..5}.html`,
`normal-flow/block-in-inline-hittest-float-002.html`,
`normal-flow/video-controls-hit-test-order.html`,
`normal-flow/hit-test-anonymous-block.html`,
`normal-flow/block-in-inline-hittest-002.html` — тоже `elementsFromPoint`,
`normal-flow/block-in-inline-hittest-001.html`), плюс два случая, где вызов
происходит на верхнем уровне `<script>` до регистрации теста —
`normal-flow/block-in-inline-hittest-relpos-zindex.html`/
`block-in-inline-hittest-margin.html` — из-за чего гарнес не завершается вовсе
(TIMEOUT, не FAIL).

`grep -n "elementFromPoint" crates/js/src/dom.rs` не находит ни одной
реализации — метод отсутствует целиком, не сломанный геттер.

## Влияние вне WPT

`document.elementFromPoint`/`elementsFromPoint` — стандартный CSSOM View API
для попадания в точку (курсор/тач/drag-and-drop хит-тесты, контекстные меню).
Отсутствие ломает любой код, определяющий элемент под курсором без ручного
геометрического перебора.

## .ini

`tests/wpt/metadata/css/CSS2/{floats/hit-test-floats-001,...-005,normal-flow/block-in-inline-hittest-float-002,normal-flow/video-controls-hit-test-order,normal-flow/hit-test-anonymous-block,normal-flow/block-in-inline-hittest-002,normal-flow/block-in-inline-hittest-001,normal-flow/block-in-inline-hittest-relpos-zindex,normal-flow/block-in-inline-hittest-margin}.html.ini`
— `expected: FAIL` (два последних — `expected: TIMEOUT` на уровне теста).
Не тронуты этим срезом — обновление ожиданий требует свежего `run_report.py`.

## Fix (P3, 2026-09-01)

Дубликат — тот же гэп независимо найден и подробнее расписан в
[BUG-477](BUG-477-DUPLICATE.md), закрыт тем же коммитом. BUG-464 выживает
как первый по дате (оба заведены 2026-08-02, BUG-464 получил номер раньше в
том же прогоне).

Новый `lumen_paint::hit_test_all` (`crates/engine/paint/src/hit_test.rs`) —
та же stacking-aware группировка (z-index-группы/transform-инверсия/
`pointer-events`/`display`-фильтры), что и одиночный `hit_test`, уже
используемый шеллом для click/cursor dispatch, но без early-return на первом
попадании: собирает ВСЕ попадания в топ-слой-первом порядке, включая цепочку
предков и перекрывающихся siblings.

Шелл пробрасывает `LayoutBox`-дерево в `crates/js` новым
`Arc<Mutex<Option<Arc<LayoutBox>>>>` (`V8JsRuntime::hit_test_tree`,
`update_hit_test_tree`), обновляемым в тех же 6 местах, что и уже
существующий `layout_rects` (`relayout.rs`, `frames.rs`, `hibernation.rs`,
трижды в `page_load.rs` через `JsLayoutSnapshot`/`page_pipeline.rs`,
`scripts.rs`) — та же геометрия и каданс, что уже видит
`getBoundingClientRect`, так что результат хит-теста гарантированно с ним
согласован.

Два новых нейтива в `install_point_hit_test`
(`crates/js/src/v8_runtime/install/platform.rs`):
`_lumen_element_from_point(x, y) -> Option<u32>` и
`_lumen_elements_from_point(x, y) -> Vec<u32>` (с дедупликацией по `NodeId`,
сохраняя порядок первого вхождения). Шим —
`document.elementFromPoint`/`elementsFromPoint`
(`crates/js/src/shim/web_api_shim_mid.js`), рядом с уже существующим
`caretPositionFromPoint`.

Тесты: 4 новых в `hit_test.rs` (предки topmost-first, перекрывающиеся
siblings по z-index, `pointer-events:none` исключён из результата, пустой
результат вне viewport) + 6 в новом `crates/js/src/dom/tests/v8_point_hit_test.rs`
(реальный layout через `lumen_layout::layout`, включая случай «до первого
layout — `null`/`[]`, не паника», и согласованность `elementFromPoint`/
`elementsFromPoint[0]`).

`.ini`-файлы WPT (10 файлов `css/CSS2/*` этого бага + ~90 файлов
`css/cssom-view`/`css-overflow`/`css-transforms`/`css-flexbox` из BUG-477)
намеренно НЕ тронуты — обновление ожиданий требует свежего `run_report.py`,
не сделанного в этой сессии.
