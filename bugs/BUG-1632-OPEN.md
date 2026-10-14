# BUG-1632 — нет `window.orientation` и `onorientationchange` (Window, HTMLBodyElement)

**Статус:** OPEN
**Заведён:** 2026-10-14 (P2, прогон `compat/` — COMPAT-CSS-WPT)
**Область:** js/dom (WebIDL-привязки Window и HTMLBodyElement, Compat Standard)

## Симптом

`tests/wpt/compat/idlharness.window.js` — 23/29. Шесть подтестов падают:

- `Window interface: attribute orientation`, `…window must inherit property "orientation"…` —
  `assert_own_property: The global object must have a property "orientation"`;
- `Window interface: attribute onorientationchange`, `…window must inherit property "onorientationchange"…`;
- `HTMLBodyElement interface: attribute onorientationchange`,
  `…document.body must inherit property "onorientationchange"` — нет свойства в цепочке прототипов.

Compat Standard описывает `Window.orientation` (число: 0 / 90 / −90 / 180) и обработчик `onorientationchange`
как legacy-атрибуты. Сайты под мобильный Chrome читают `window.orientation` для выбора раскладки.

## Что нужно

1. Добавить `orientation` (десктоп: 0) и `onorientationchange` в Window и HTMLBodyElement (как window-reflecting
   обработчик событий).
2. Проверка: `python tests/wpt/run_corpus.py --prefixes compat` — `idlharness.window` 29/29.
