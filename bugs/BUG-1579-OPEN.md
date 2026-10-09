# BUG-1579 — Селекторы `:host`: потомок и дочерний комбинатор после `:host(...)`, `:is(:host)`/`:where(:host)`, `* :host`, `:host(:has())`, `:host-context()`

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** css-parser/layout (`crates/engine/css-parser`, `crates/engine/layout/src/selector_query.rs` — сопоставление `:host` в shadow-контексте)

## Симптом

Селектор `:host` распознаётся только как самостоятельный составной (`:host`, `:host(<selector>)` с хвостом из других простых селекторов). Любой комбинатор справа (`:host div`, `:host > div`) или оборачивание в `:is()`/`:where()` ломает совпадение; `* :host` совпадает ошибочно.

## Проба

`--dump-layout` + `console.log`, `div#h` с shadow root, в нём `<style>`, `<div id=in>`, замер `getComputedStyle(in).width` (правило `{width:100px}`; без правила — 384px):

| правило в shadow `<style>` | у нас | ожидается |
|---|---|---|
| `:host div` | 384px | 100px |
| `:host > div` | 384px | 100px |
| `:host(.a) div` | 384px | 100px |
| `:host > *`, `:host *` | 384px | 100px |
| `div` (контроль) | 100px | 100px |
| `:host{width:100px;display:block}` (на хосте) | 100px | 100px |
| `:host(.a)`, `:host(:not(.b))`, `:host(:is(.a))`, `:host(:has(div))`, `:host:has(div)` (на хосте) | 100px | 100px |
| `:is(:host){…}`, `:where(:host){…}` (на хосте) | 384px | 100px |
| `:host{width:100px;…} * :host{width:50px}` (на хосте) | 50px | 100px |
| `:host-context(body){…}` | не совпало | совпало; `CSS.supports("selector(:host-context(body))")` — `false` |
| `attachShadow`: `:host(:has(section)) div{background:green}`, `<section>` в светлом дереве хоста | красный | зелёный |
| `attachShadow`: `:host:has(section) div{…}`, `<section>` в shadow tree | красный | зелёный |
| declarative shadow DOM (`<template shadowrootmode>`) строит дерево верно (контроль) | `shadowRoot` есть, 2 ребёнка | то же |

## Как найдено

WPT-RUN-14 срез 26: `css/css-shadow/host-*` (`host-descendant-001…003`, `host-is-001/002/005/006`, `host-has-*`, `host-defined`, `host-multiple-003/004`, `host-not-001`, `host-nested-001`, `host-slotted-001`, `host-specificity-*`), `host-context-*` (5 id). Часть reftest-ов упирается в BUG-1580 (каскад областей) — отдельно не разделено.

## Что делать

Сопоставлять `:host` как «корневой» элемент контекста shadow tree при любом положении в составном/сложном селекторе, поддержать его внутри `:is()`/`:where()`/`:has()`, не сопоставлять `* :host`; разобрать `:host-context()`.

## Как проверить

`css/css-shadow/host-descendant-001.html`, `host-is-001.html`, `host-context-specificity-001.html`.
