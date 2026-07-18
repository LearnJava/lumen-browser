# BUG-315: `MutationRecord` не выставлен как глобальный интерфейс

**Статус:** OPEN
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S6, курируемый асинхронный DOM-сабсет через `wptrunner`

## Симптом

Интерфейс `MutationRecord` не выставлен на глобальном объекте — `MutationRecord
is not defined`. Колбэк MutationObserver вызывается корректно (асинхронно, через
microtask), но передаваемые записи нельзя проверить через `instanceof
MutationRecord`.

Наблюдаемый провал:

- `dom/nodes/MutationObserver-callback-arguments.html` →
  `Callback is invoked with |this| value of MutationObserver and two arguments`
  → `MutationRecord is not defined` (`expected: FAIL`).

Та же семья, что [BUG-314](BUG-314-OPEN.md) (DOM-конструкторы не выставлены как
глобали).

## Ожидание

DOM Standard §4.3.3: `MutationRecord` доступен как глобальный интерфейс;
записи, передаваемые в колбэк `MutationObserver`, — его экземпляры. Реализовать
в engine-agnostic `WEB_API_SHIM`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/MutationObserver-callback-arguments.html
```
