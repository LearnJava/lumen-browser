# BUG-1147 — кросс-фреймовый Element-фасад не определяет `.style`/`classList`/`dataset`

**Статус:** OPEN
**Заведён:** 2026-09-24 (w3schools, разбор совместимости top100; передан P6 по решению пользователя).
**Область:** js (`crates/js/src/frame_bridge.rs::frameElem`)

## Симптом

Тот же фасад `frameElem`, что чинился в [BUG-970](BUG-970-FIXED.md) (`.attributes`), не определяет
`.style` (и `classList`, `dataset`): `iframe.contentDocument.documentElement.style` и `body.style` —
`undefined`. w3schools (FastCMP, `fast-cmp-en-tcfeuv2.js:1:174891`):
`Cannot set properties of undefined (setting 'cssText')` — диалог согласия на cookie не строится.
Репро `.tmp/compat/g6/site/iframedoc.html` (iframe без `src`): Lumen `typeof style === 'undefined'` →
`TypeError`; Chrome отдаёт `CSSStyleDeclaration`, `cssText` применяется, конструктор
`HTMLHtmlElement`.

## Масштаб

Тот же класс пробела, что закрыт для `.attributes` в BUG-970 — фасад покрывает широкую
поверхность (`nodeType`/`tagName`/`getAttribute`/`children`/`querySelector`/…), но не инлайновые
стили/классы/data-атрибуты чужого поддокумента.
