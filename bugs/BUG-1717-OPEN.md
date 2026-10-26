# BUG-1717 — vkvideo.ru: цикл `getBoundingClientRect` в `initVideoElementAbrContext` держит поток движка до 5 с

**Статус:** OPEN
**Заведён:** 2026-10-26 (P6, живая проверка после BUG-1714)
**Область:** js/layout (`getBoundingClientRect` → `DOMRect` в `web_api_shim_mid.js`; стек `[js-stall]`: `DOMRect` ← `getBoundingClientRect` ← `<anon>` ← `subscribe` ← `connect` ← `initVideoElementAbrContext`)

## Симптом

`https://vkvideo.ru/video-213276384_456243633`, `dev-release`, `--maximized`, `LUMEN_NO_ADBLOCK=1`, без прокси, `LUMEN_JS_STALL_SAMPLE_MS=1000`. После BUG-1714 стопоры от `<link>`-подсказок ушли, остались задачи 3–5 с с кадром `DOMRect (…)` на вершине стека и `initVideoElementAbrContext` внизу; MCP `eval` в это время отвечает за 7–8 с. Пока не ясно, плеер зовёт `getBoundingClientRect` в цикле или каждый вызов дорогой (принудительный relayout).

## Что делать

Свести к странице-пробе: посчитать число вызовов `getBoundingClientRect` и стоимость одного в задаче; если дорог принудительный relayout — искать, почему DOM не чист между вызовами; если вызовов тысячи — сверить с Chrome.
