# BUG-1790 — Cloudflare Turnstile: челлендж-кадр работает, но не выдаёт токен (детект со стороны challenge)

**Статус:** OPEN (срез 1 — исправлены два дефекта ниже, токен всё ещё не выдаётся)
**Заведён:** 2026-11-06 (P3, остаток [BUG-1595](BUG-1595-FIXED.md))
**Область:** js/shell — антибот-фингерпринт (`window.chrome`, `navigator.plugins`/`mimeTypes`), `MessageEvent.isTrusted` (UX-ANTIBOT)

## Срез 1 (2026-11-15, P3)

Стенд переписан: `.tmp/turnstile.html` (локальная страница, ключ
`1x00000000000000000000AA`, `data-callback`/`data-error-callback` пишут результат
в `#st`) + `.tmp/turnstile_verify.py` (Lumen, `--bidi-port`, опрос `#st`) и
`.tmp/turnstile_chrome.py` (Chrome for Testing, плечо-эталон: `TOKEN` за 2 с).
Дифф-проба фингерпринта — `.tmp/fp1790.js`/`fp1790.py` + `fp1790b.js`.

Найдено и исправлено:

1. **`window.chrome` отсутствовал** — добавлена форма настоящего Chrome
   (`loadTimes`, `csi`, `app`, без `runtime`; ADR-034 п.1), и `navigator.plugins`/
   `mimeTypes` больше не пустые объекты, а `PluginArray` (5 PDF-плагинов) и
   `MimeTypeArray` (2 типа) с интерфейсами `Plugin`/`MimeType` — как мерил Chrome
   151 и как требует ADR-007 слой 1 (`crates/js/src/surface_api.rs`).
2. **`MessageEvent.isTrusted` был `false`** на обоих путях `postMessage`
   (`window.postMessage` и доставка из бриджа фреймов) — Chrome отдаёт `true`.
   Cloudflare Turnstile отбрасывает любое сообщение с `!e.isTrusted`
   (`Tu = e.isTrusted && …` в `api.js`), поэтому `init`/`requestExtraParams`
   до родителя доходили, но ответа кадр не получал: `Ignored message from
   unexpected source` / «widget hung» 300030 (`web_api_shim_mid_b4.js`,
   тест `post_message_and_frame_messages_are_trusted`).

После обоих фиксов родитель сообщения принимает (в логе нет
`Ignored message …`), но токена по-прежнему нет: `complete` не приходит,
через ~45 с — `ERROR 300030`. Остаток локализован до **ответа родителя в кадр**:

- `api.js`: `ve(Xe, msg, o.iframeOrigin)` → `contentWindow.postMessage(msg, "https://challenges.cloudflare.com")`;
  `Zt()` молча выходит, если `iframeOrigin` пуст, а `contentWindow` не имеет
  `postMessage`. Исключений в родителе нет (ловушка `window.onerror` пуста),
  кадр ответа не получает.
- Проверки на стенде: `Fe(...).contentWindow` из `Su` (= та же проверка, что
  делает `api.js`) отдаёт тот же фасад, что `event.source`; `targetOrigin`
  кадра — `https://challenges.cloudflare.com` (`new URL(...).origin` в Lumen
  считает верно). Дальше искать: `JSON.stringify` ответа (`wPr` содержит DOM-узлы:
  `document.body.parentElement`, `Ea(o.wrapper)`, `Oa/Aa(...)`) или проверку
  `targetOrigin`/`resolve_slot` в `_lumen_f_post_message` для cross-origin
  sandbox-кадра.
- Второй срез должен начинаться с запуска `_lumen_f_post_message` из бриджа
  фреймов **в реальном сценарии ответа** (в контексте страницы этот натив не
  виден: `window._lumen_f_post_message === undefined`, обёртка не ловит вызовы —
  он живёт в замыкании шима бриджа).


## Симптом

Страница `https://peet.ws/turnstile-test/non-interactive.html`, окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, ключ `1x00000000000000000000AA` (non-interactive, всегда pass):

- `turnstile.render()` возвращает id, challenge iframe создаётся и грузится (`GET cdn-cgi/challenge-platform/...` → 200).
- Скрипт кадра исполняется, `postMessage` доходит до родителя (`init`, `requestExtraParams`, `food`-heartbeat, `overrunBegin`/`overrunEnd`).
- `callback`/`error-callback` не дают токена: error-callback получает `"300030"` (watchdog «widget hung», ~40 с без ack), челлендж крутит overrun-ретраи без `complete`/`fail`, JS-исключений в кадре нет (console фрейма дренируется с BUG-1595 — чисто).
- В Chrome тот же стенд выдаёт токен.

## Что уже исключено

- Дефект iframe/shadow DOM — исправлен в [BUG-1595](BUG-1595-FIXED.md): iframe в closed shadow root теперь browsing context, грузится и исполняется.
- Сеть — все challenge-запросы 200, CSP/adblock не при чём (`LUMEN_NO_ADBLOCK=1`).
- Исключения JS в кадре — console фрейма дренируется в DevTools/health-log, записей нет.

## Гипотезы (по приоритету проверки)

1. ~~**`window.chrome` отсутствует**~~ — сделано в срезе 1 (форма Chrome без `runtime`).
2. ~~**`navigator.plugins`/`mimeTypes` пусты**~~ — сделано в срезе 1 (PDF-набор Chrome).
3. ~~**`MessageEvent.isTrusted === false`**~~ — сделано в срезе 1.
4. Открыто: ответ родителя (`extraParams`) не доходит до кадра или не принимается им — см. «Срез 1».

## Как проверять

- Стенд: `python .tmp/turnstile_verify.py <path-to-lumen.exe>` (прогон ~90 с, ждёт `cf-turnstile-response`/callback).
- Дифф-проба: снять тот же набор фингерпринтов в Chrome for Testing и в Lumen (`navigator.webdriver`, `window.chrome`, `plugins`, `mimeTypes`, `userAgentData`, `hardwareConcurrency`, `deviceMemory`, WebGL UNMASKED_*) и сравнить.
- Если фингерпринт выровняется, а токен всё равно не приходит — смотреть heartbeat-ack: `food`-сообщения доходят, но ответ кадра может теряться на `postMessage` targetOrigin/origin-проверках (у sandbox-кадра opaque origin).

## Связи

- Родитель: [BUG-1595](BUG-1595-FIXED.md) — iframe в shadow root.
- Смежно: [BUG-754](BUG-754-FIXED.md) (`window.chrome`), UX-ANTIBOT-дорожка.

## Источник: сайты (дополнение SITES-TRIAGE-3, 2026-11-13)

medium-com (вес 3): базовый прогон — HUNG, перепрогон 2026-11-13 — run1 403 + `[Cloudflare Turnstile] Unhandled error: Cannot read properties of null (reading 'eval')`, SITE_REFUSED; Chrome проходит (229 знаков — челлендж, который Chrome решает). Тот же debug-info `IQ4G/PW4hbx…`, что в симптоме выше. vimeo-com тоже ловит Turnstile-ошибку, но Chrome там получает тот же 403 — не наш.
