# BUG-1511 — Scoped View Transitions (`element.startViewTransition()`): метода нет, 55 тестов `scoped/` падают на первом вызове

**Статус:** OPEN (ДОРАБОТКА → VT-SCOPED)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** js (`crates/js/src/view_transitions.rs` — `Element.prototype.startViewTransition`)

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
