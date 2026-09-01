# BUG-875 — `window.visualViewport` отсутствует целиком

**Статус:** OPEN (ДОРАБОТКА → [GAP-VVPORT](../ROADMAP.md))
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
