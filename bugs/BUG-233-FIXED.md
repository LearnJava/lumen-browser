# BUG-233

**Статус:** FIXED 2026-06-22
**Компонент:** js (shell)
**Файл:** `crates/js/src/dom.rs` (конец `WEB_API_SHIM`, область инсталляции DOM-шима)

## Описание

Глобал `self` не определён в JS-окружении. В браузере `self === window === globalThis`
(WindowOrWorkerGlobalScope); webpack-рантайм и многие библиотеки используют `self` как
ссылку на глобальный объект (`(self.webpackChunk = self.webpackChunk || []).push(...)`,
`typeof self !== 'undefined' ? self : this`). В Lumen DOM-шим определяет отдельный
объект-литерал `window` ([dom.rs:8288](../crates/js/src/dom.rs)), но **не** алиасил его
на `self`/`globalThis`. Все `var self = this` в шиме — локальные function-scope алиасы,
глобального `self` не было.

Следствие — на любом сайте с webpack-бандлом первый же чанк падал:

```
script error: JS runtime error: self is not defined      (×4 на lenta.ru)
script error: JS runtime error: cannot read property 'length' of undefined
script error: JS runtime error: not a function
script error: JS runtime error: no setter for property   (×2)
```

`self is not defined` — корневая ошибка; остальные (`not a function`,
`'length' of undefined`) — каскад: webpack-рантайм не инициализировал свой
chunk-реестр, дальнейший код бандла работал с `undefined`.

## Как починено

В конце `WEB_API_SHIM` (после того как `window` собран и пополнен конструкторами)
добавлен блок алиасинга: `self`/`window`/`globalThis` указывают на один объект, как в
браузере:

```js
var self = window;
globalThis.self      = window;
globalThis.window    = window;
window.self          = window;
window.window        = window;     // window.window === window (HTML LS)
window.globalThis    = globalThis;
window.frames        = window;
window.top           = window;
window.parent        = window;
window.length        = 0;          // число дочерних browsing contexts (фреймов)
```

`self` и `window` — **один и тот же объект-ссылка**, поэтому свойства, положенные
бандлом в `self` (chunk-реестр), видны и через `window`, и наоборот.

Регрессионные тесты в `crates/js/src/dom.rs`:
- `self_window_globalthis_are_the_same_object` — `self === window`, `window.top/parent/frames === window`, `globalThis.self === window`.
- `self_and_window_share_property_storage` — `self.webpackChunk.push(...)` виден через `window.webpackChunk`.

`no setter for property` — отдельный дефект (присваивание read-only accessor-свойству
шима), не связан с `self`; отслеживается отдельно при необходимости.

## Контекст

Найдено при замере скорости загрузки vs Edge (lenta.ru, 2026-06-22). См. также
BUG-234 (нет HTTP-кэша) — второй фактор «долго грузится». Полноценный JS-паритет
upstream закрывается переходом на V8 (rusty_v8) в Фазе 3 (`P3-v8`); `self`-алиас
тривиален и сделан сейчас в QuickJS-шиме.
