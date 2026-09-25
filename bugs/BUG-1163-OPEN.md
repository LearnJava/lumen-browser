# BUG-1163 — `Text.prototype.wholeText` отсутствует

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** js — `crates/js/src/shim/*.js`: `grep -rn wholeText crates/js/src` — ноль совпадений

## Симптом

DOM §4.11: `wholeText` — данные всех смежных текстовых узлов. У Lumen свойство `undefined`.
`dom/nodes/Node-properties.html` (2026-09-25, после BUG-863):

```
FAIL xmlTextNode.wholeText     - expected (string) "do re mi fa so la ti" but got (undefined) undefined
FAIL foreignTextNode.wholeText - expected (string) "I admit that …" but got (undefined) undefined
```

и так для каждого текстового узла в наборе `setupRangeTests()`.

## Что чинить

Геттер на `Text.prototype` (и в обёрточных дескрипторах CharacterData): склеить данные
предыдущих и следующих соседей, пока они Text-узлы.
