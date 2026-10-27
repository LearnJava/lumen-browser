# BUG-1729 — путь события (`composedPath()`) не идёт через слоты и теневые деревья: slotted-узел видит только свой light-путь

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-SHADOW-SCOUT)
**Область:** js (`crates/js/src/shim/event_shim.js`, построение пути диспатча и `Event.prototype.composedPath`, ~стр. 36–40; диспатч в `web_api_shim_mid.js`)

## Симптом

```
<div id=h1><span id=lt slot=s></span></div>;  sr = h1.attachShadow({mode:'open'});
sr.innerHTML = '<div id=in><slot name=s id=sl></slot></div>';
lt.addEventListener('y', e => path = e.composedPath().map(n => n.id || n.nodeName));
lt.dispatchEvent(new Event('y', {bubbles:true, composed:true}));
path → lt > h1 > BODY > HTML > #document > Window
```

Ожидание (DOM LS §2.9 «get the parent» + assigned slot): `lt > sl > in > #shadow-root > h1 > BODY > HTML > #document > Window`. Родитель slotted-узла — его `assignedSlot`, поэтому событие проходит через слот и теневое дерево хоста; `relatedTarget`-ретаргетинг считает путь так же.

## Как найдено

WPT-SHADOW-SCOUT (кластер F): `event-inside-slotted-node.html` 0/20 («EventPath must contain [target, target parent, slot, slot parent, shadow root, shadow host]»), `event-with-related-target.html` 0/18 (`["BNa","BNc-S","BN-SR","BN","B-SR"]` ожидалось, пришло `["BNa","BN","B-SR"]`), `Extensions-to-Event-Interface.html` 8/16, `event-composed-path-with-related-target.html`, `capturing-and-bubbling-event-listeners-across-shadow-trees.html`.

## Что делать

В построении пути диспатча родитель узла: если у узла есть `assignedSlot` — слот; для `ShadowRoot` — `host` (при `composed` или если событие не создано внутри этого корня). Закрытые корни: путь для слушателей снаружи скрывается по правилам `composedPath()`, а не выпадением узлов из пути.

## Как проверить

Проба выше; `run_corpus.py --prefixes shadow-dom` — файлы `event-*` вне кластера E (после [BUG-1728](BUG-1728-OPEN.md)) ≥ 90 % сабтестов.
