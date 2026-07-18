# BUG-311: Node.isConnected отсутствует в DOM-шиме

**Статус:** OPEN
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
