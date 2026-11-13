# BUG-1790 — Cloudflare Turnstile: челлендж-кадр работает, но не выдаёт токен (детект со стороны challenge)

**Статус:** OPEN
**Заведён:** 2026-11-06 (P3, остаток [BUG-1595](BUG-1595-FIXED.md))
**Область:** js/shell — антибот-фингерпринт (`window.chrome`, `navigator.plugins`/`mimeTypes`, UX-ANTIBOT)

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

1. **`window.chrome` отсутствует** — намеренно убран по [BUG-754](BUG-754-FIXED.md); challenge-платформа Cloudflare проверяет `chrome.app`/`chrome.runtime`/`chrome.csi` — главный кандидат.
2. **`navigator.plugins`/`mimeTypes` пусты** (length 0) — у Chrome там 5 PDF-плагинов; тривиальный детект.
3. Прочие фингерпринт-пробелы: `navigator.userAgentData`, WebGL renderer/vendor, `screen`/`devicePixelRatio` согласованность, отсутствие `chrome`-специфичных API, timing/behavioral (challenge измеряет rAF/таймеры).

## Как проверять

- Стенд: `python .tmp/turnstile_verify.py <path-to-lumen.exe>` (прогон ~90 с, ждёт `cf-turnstile-response`/callback).
- Дифф-проба: снять тот же набор фингерпринтов в Chrome for Testing и в Lumen (`navigator.webdriver`, `window.chrome`, `plugins`, `mimeTypes`, `userAgentData`, `hardwareConcurrency`, `deviceMemory`, WebGL UNMASKED_*) и сравнить.
- Если фингерпринт выровняется, а токен всё равно не приходит — смотреть heartbeat-ack: `food`-сообщения доходят, но ответ кадра может теряться на `postMessage` targetOrigin/origin-проверках (у sandbox-кадра opaque origin).

## Связи

- Родитель: [BUG-1595](BUG-1595-FIXED.md) — iframe в shadow root.
- Смежно: [BUG-754](BUG-754-FIXED.md) (`window.chrome`), UX-ANTIBOT-дорожка.

## Источник: сайты (дополнение SITES-TRIAGE-3, 2026-11-13)

medium-com (вес 3): базовый прогон — HUNG, перепрогон 2026-11-13 — run1 403 + `[Cloudflare Turnstile] Unhandled error: Cannot read properties of null (reading 'eval')`, SITE_REFUSED; Chrome проходит (229 знаков — челлендж, который Chrome решает). Тот же debug-info `IQ4G/PW4hbx…`, что в симптоме выше. vimeo-com тоже ловит Turnstile-ошибку, но Chrome там получает тот же 403 — не наш.
