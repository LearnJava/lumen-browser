# BUG-311: Node.isConnected отсутствует в DOM-шиме

**Статус:** FIXED 2026-07-19
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S5, курируемый синхронный DOM-сабсет через `wptrunner`

## Симптом

`Node.prototype.isConnected` (DOM Standard §4.4) отсутствует —
`grep isConnected crates/js/src/dom.rs` → 0 совпадений. Свойство возвращает
`undefined` вместо булева.

Провалы сабтестов `Node-isConnected.html` (оба `expected: FAIL`):

```
Test with ordinary child nodes  -> assert_false: expected false got undefined
Test with iframes               -> assert_false: expected false got undefined
```

## Ожидание

`isConnected` — геттер на `Node.prototype`: `true`, если узел в дереве,
корень которого — документ (shadow-inclusive). В нашем шиме достаточно
проверки достижимости `document` по цепочке `parentNode`. Реализовать в
engine-agnostic `WEB_API_SHIM`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/Node-isConnected.html
```

## Решение (2026-07-19)

Геттер `isConnected` добавлен в `WEB_API_SHIM` внутри `_lumen_make_element`
(engine-agnostic путь, оба движка). Реализация в терминах уже имеющихся
нативов: узел подключён, если `documentElement` (`<html>`, через
`_lumen_get_html_element`) лежит на цепочке предков (`_lumen_get_parent`) или
является самим узлом. Отсоединённое поддерево до `<html>` не доходит — его
верхний предок это узел-сирота, поэтому `isConnected === false`. После
`remove()` узел снова рапортует `false`.

Сабтест «Test with ordinary child nodes» → PASS (метадата
`Node-isConnected.html.ini` обновлена). «Test with iframes» остаётся FAIL:
он опирается на отдельные под-документы iframe через `contentDocument`,
которые шим не моделирует как самостоятельные подключённые деревья
(независимый пробел движка).

Юнит-тест: `node_is_connected_reflects_document_attachment` (detached →
false, его detached-потомок → false, attached → true, `documentElement` →
true, после `remove()` → false).
