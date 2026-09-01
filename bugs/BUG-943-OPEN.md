# BUG-943 — `Element.currentCSSZoom` не существует: CSS Zoom OM отсутствует целиком

**Статус:** OPEN
**Тип:** нереализованная функциональность, не дефект реализованного кода — член IDL нигде не заведён.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31)
**Область:** js (`crates/js/src/shim/*.js` — нет ни одного `currentCSSZoom`/`currentCSSTranslate`/`currentCSSRotate`/`currentCSSScale` в шиме), layout (`ComputedStyle::zoom` существует и применяется к разметке, но ничего не публикует наружу для CSSOM-читателя)
**Владелец:** P3.

## Симптом

`getComputedStyle(el).currentCSSZoom` — `undefined` вместо числа: workspace-wide
`grep -rn currentCSSZoom` даёт ноль совпадений в `crates/`. Спецификация — CSS
Values and Units L4 §CSS Zoom Object Model: `Element` получает
`currentCSSZoom` (эффективный `zoom` по цепочке предков), а
`ResizeObserverEntry` — не-масштабированную пару `contentBoxSize`/`devicePixel
ContentBoxSize`, отличную от масштабированного `contentRect`.

## Прямое измерение

Workspace-wide `grep -rn currentCSSZoom crates/` — ноль совпадений. `zoom`
как CSS-свойство существует и парсится (`crates/engine/layout/src/style.rs`),
но нет геттера, который отдавал бы его эффективное (накопленное по предкам)
значение в JS.

## Кого это держит

`resize-observer/zoom.html` — тест проверяет, что `ResizeObserverEntry`
различает масштабированный и немасштабированный размер именно через
`currentCSSZoom`; без члена тест падает на первой же попытке его прочитать.

## Направление починки

1. Посчитать эффективный `zoom` элемента (произведение `zoom` по цепочке
   предков, как это уже делает layout при расчёте геометрии) и завести
   геттер `currentCSSZoom` на `Element.prototype`.
2. Дать `ResizeObserverEntry` немасштабированную пару размеров — сейчас
   `contentBoxSize`/`contentRect` уже масштабированы geometry-путём, нужен
   второй расчёт с `zoom = 1`.

Небольшой, самодостаточный кусок работы — один читающий член плюс один
альтернативный расчёт размера, не новая подсистема.
