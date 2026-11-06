# BUG-1595 — Cloudflare Turnstile: `turnstile.render` возвращает id, iframe не создаётся

**Статус:** FIXED 2026-11-06 (P3)
**Заведён:** 2026-10-10 (P6, UX-ANTIBOT, живая проверка на капчах)
**Область:** engine/dom (`collect_iframes`), shell (console фреймов)

## Симптом

Страница `https://peet.ws/turnstile-test/non-interactive.html`, окно `--maximized`, `LUMEN_NO_ADBLOCK=1`:

- `api.js` загружался, `typeof window.turnstile === 'object'`, `turnstile.render(...)` возвращал id.
- iframe в документе не появлялся, `callback`/`error-callback` не вызывались, токена нет.

## Причина

Виджет Turnstile вставляет свой `<iframe>` **внутрь closed shadow root** (`Gt.attachShadow({mode:"closed"})` в `api.js`). `Document::collect_iframes` (`crates/engine/dom/src/lib.rs`) обходил только DOM-children: shadow root — не DOM-ребёнок хоста, поэтому iframe внутри shadow tree никогда не доходил до `IframeInfo` → browsing context не создавался → `src` выставлялся, но документ не запрашивался (в сетевом логе не было ни одного запроса к `cdn-cgi/challenge-platform`).

Диагностика: перехват `attachShadow` показал `SR.appendChild arg=IFRAME src=https://challenges.cloudflare.com/cdn-cgi/...` — вставка в shadow root была, а загрузки не было. Ручной тест подтвердил: iframe в open/closed shadow root получал `contentWindow === null`, в обычном DOM — работал.

## Исправление

- `collect_iframes_inner` после обхода DOM-children спускается в `doc.shadow_root_of(id)` — shadow-поддерево обходится наравне со светлым DOM. Светлые дети хоста уже обойдены и через `<slot>` не дублируются; UA shadow roots (`<select>`/`<details>`/`<video>`) безвредны — iframe там не бывает.
- Тест `collect_iframes_finds_iframe_inside_shadow_root` в `crates/engine/dom/src/tests/frames_events.rs`.
- Попутно: console-сообщения JS-рантаймов фреймов (`FrameHandle::js`) теперь дренируются в DevTools console/health-log (`about_to_wait.rs`) — до этого `console.error`/непойманные исключения внутри iframe исчезали бесследно. Проверено стендом: `Uncaught` из iframe виден в `resource://console`.

## Проверка после исправления

Тот же стенд: iframe создаётся в shadow root, `GET cdn-cgi/challenge-platform/...` → 200, inline-скрипт кадра исполняется, `postMessage` доходит до родителя (`init`, `requestExtraParams`, `food`-heartbeat, `overrunBegin/End`). Токен с тестовым ключом `1x00000000000000000000AA` **пока не выдаётся**: челлендж крутит `overrun`-ретраи без `complete`/`fail`, JS-исключений в кадре нет (проверено дренажем console фрейма). Это уже не дефект iframe — challenge-side детект (кандидаты: `window.chrome` намеренно отсутствует по BUG-754, `navigator.plugins`/`mimeTypes` пусты) — территория UX-ANTIBOT/фингерпринта, не этого бага.
