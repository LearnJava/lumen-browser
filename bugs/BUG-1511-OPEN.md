# BUG-1511 — Scoped View Transitions (`element.startViewTransition()`): метода нет, 55 тестов `scoped/` падают на первом вызове

**Статус:** OPEN (ДОРАБОТКА → VT-SCOPED)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** js (`crates/js/src/view_transitions.rs` — `Element.prototype.startViewTransition`)

## Срез 1

**Срез 1 влит 2026-10-24 (P3):** `Element.prototype.startViewTransition` и `Element.activeViewTransition` (`view_transitions_shim.js`: тот же автомат на каждую область; `document` и `documentElement` делят одну, перезапуск области пропускает её текущий переход, области идут параллельно, колбэки — в порядке создания), `transitionRoot` = область, свойство `view-transition-scope: none | all` (разбор, css-wide, `getComputedStyle` → `all` у области бегущего перехода), обнаружение имён внутри поддерева области до первого `view-transition-scope: all` (натив `_lumen_vt_scope_roots`), элемент, захваченный бегущим переходом другой области, → `InvalidStateError`, дерево псевдоэлементов области в `getComputedStyle(scope, '::view-transition…')` и `getAnimations()` (цель — элемент области), `finished` области ждёт анимации её дерева, `ready` пропущенного перехода помечен обработанным. Попутно: разбор `<script>/<style>` между `</head>` и `<body>` (тело оставалось вне дерева, `document.body.isConnected` был `false`) и CSS-wide ключевые слова для `animation*` (`animation: unset` ничего не сбрасывал). WPT `scoped/`: 10 crashtest TIMEOUT → PASS (14 из 14), тестов `.html` без reftest 10 из 20 id зелёные (`capture`, `callback-order`, `collision`, `element-active-view-transition`, `start-view-transition`, `scope-during-transition-1`, `view-transition-scope-*`, `negative-playback-rate`); 27 из 40 подтестов без reftest. Остаток — VT-SCOPED-2: пиксели области (снимок и кросс-фейд рисует шелл только для документа, 32 reftest), `auto`/`match-element` как несколько групп с одним именем, значения анимаций в вычисленном стиле псевдо (`run_in_parallel`), `-ua-mix-blend-mode-plus-lighter`, пропуск при `display: none` области, hit-testing, корневое `view-transition-name: none` у `:root` (`document-element-start-view-transition`).

## Симптом

`typeof Element.prototype.startViewTransition` — `undefined`. Тесты `css-view-transitions/scoped/*` вызывают `target.startViewTransition(…)` и получают `TypeError: target.startViewTransition is not a function`; 10 crashtest из них ждут снятия `test-wait` после вызова и упираются в таймаут (исключение до снятия класса). Спецификация — черновик (`css-view-transitions-2`, scoped transitions), поддержка в одном движке; решение «делать ли» — за пользователем.

## Проба

Проба (`--mcp`): `typeof document.documentElement.startViewTransition` → `undefined`; `typeof document.startViewTransition` → `function`.

## Как найдено

WPT-RUN-14 срез 22: `css-view-transitions/scoped/*` — 55 id (20 OK с падениями, 25 FAIL — из них 24 с `reftest-wait`, 10 TIMEOUT — crashtest).

## Что делать

Реализовать после VT-API (BUG-1510): метод на `Element`, область перехода (scope), захват и анимация внутри поддерева.

## Как проверить

`css/css-view-transitions/scoped/capture.html`.
