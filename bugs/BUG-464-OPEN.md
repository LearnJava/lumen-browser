# BUG-464: `document.elementFromPoint`/`elementsFromPoint` not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — `var document = {...}` литерал; CSSOM
View §3)
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
