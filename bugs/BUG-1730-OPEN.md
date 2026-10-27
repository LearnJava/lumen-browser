# BUG-1730 — `slotchange` приходит синхронно и не по спеке; `assignedNodes({flatten:true})` не отдаёт fallback-содержимое пустого слота

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-SHADOW-SCOUT)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js`: поиск `slotchange`, `assignedNodes`/`assignedElements`; сигнал `signal a slot change` — DOM LS §4.2.2.5)

## Симптом

Проба `lumen --dump-layout`:

```
sr.getElementById('sl').addEventListener('slotchange', () => n++);
h.appendChild(<b slot="s">);  n   → 1 сразу после appendChild   (ожидание: 0 — событие в mutation-observer microtask, не синхронно)
```

`slotchange-event.html` — 0/32: «slotchange event must not be fired synchronously» (10), «must be fired exactly once after the assigned nodes changed» (4), «…if the assigned nodes are changed by innerHTML» (4), событие при вставке слота с уже назначенными узлами (4). Отдельно:

```
r = div.attachShadow({mode:'open'}); r.innerHTML = '<slot>fb</slot>';
r.firstChild.assignedNodes({flatten:true}).length   → 0   (ожидание 1 — текст «fb»: у слота нет назначенных узлов, flatten отдаёт fallback)
```

(`HTMLSlotElement-interface.html` 14/18, `slots-fallback.html`).

## Как найдено

WPT-SHADOW-SCOUT (кластер G + остаток N, `docs/wpt-vendor-notes/shadow-dom.md`): 2 id × 33 сабтеста + `HTMLSlotElement-interface.html`, `slots-fallback*.html` (последние ещё закрыты [BUG-1728](BUG-1728-OPEN.md)).

## Что делать

(1) `slotchange` — поставить слот в «signal slots» и диспатчить из microtask mutation observer (один раз на слот за пачку), `bubbles: true`, `composed: false`; (2) `assignedNodes({flatten:true})`/`assignedElements({flatten:true})` — рекурсивно по правилу «find flattened slottables» (fallback у слота без назначений, вложенные слоты).

## Как проверить

Проба выше; `slotchange-event.html` ≥ 28/32, `HTMLSlotElement-interface.html` 18/18.
