# BUG-1052 — `zoom: calc(1 + (sign(...) * 0.5))` — `sign()` math function делает всё значение `zoom` невалидным

**Статус:** OPEN
**Заведён:** 2026-09-13 (BUG-532 срез P3, живой прогон `css/css-viewport/zoom/zoom-with-sign-function.html`)
**Область:** css-parser (`zoom` grammar / `calc()` `sign()` math function)
**Владелец:** P3/P4

## Симптом

`run_smoke.py //css/css-viewport/zoom/zoom-with-sign-function.html`
(dev-release, 2026-09-13) — 0/2:

```
FAIL calc(sign(1em - 1px) * 2) should be used-value-equivalent to 2 -
  assert_not_equals: calc(sign(1em - 1px) * 2) isn't valid in 'zoom';
  got the default value instead. got disallowed value ""
FAIL calc(sign(1em - 1px) * 2%) should be used-value-equivalent to 2% - (то же)
```

`cascade.rs::parse_zoom` не распознаёт `calc()`-выражения с `sign()`
математической функцией внутри — всё значение отбрасывается как невалидное
и `zoom` остаётся на дефолте, вместо резолва `sign()` в used-value контексте
(`1em - 1px` знак зависит от `font-size` элемента).

## Что дальше

Не локализовано глубже прогона. Проверить, распознаёт ли парсер `zoom`
(`parse_zoom` в `cascade.rs`) `calc()`-выражения вообще, или только плоские
числа/проценты — если только плоские, `sign()` — не специфичный для
`sign()` пробел, а более общий «`zoom` не принимает `calc()`».
