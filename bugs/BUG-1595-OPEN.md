# BUG-1595 — Cloudflare Turnstile: `turnstile.render` возвращает id, но виджет не создаётся

**Статус:** OPEN
**Заведён:** 2026-10-10 (P6, UX-ANTIBOT, живая проверка на капчах)
**Область:** js/shell (загрузка и исполнение `challenges.cloudflare.com/turnstile/v0/.../api.js`; причина не локализована)

## Симптом

Страница `https://peet.ws/turnstile-test/non-interactive.html`, окно `--maximized`, `LUMEN_NO_ADBLOCK=1`:

- `api.js` загружается (302 → `/turnstile/v0/g/<hash>/api.js`, 200), `typeof window.turnstile === 'object'`, в консоли `[Cloudflare Turnstile] Compatibility layer enabled`.
- Ручной вызов `turnstile.render(div, {sitekey:'1x00000000000000000000AA', callback, 'error-callback'})` (тестовый ключ «всегда проходит») возвращает id `cf-chl-widget-…`.
- Через 15 с: iframe в документе нет, `callback` не вызван, `error-callback` не вызван, `input[name=cf-turnstile-response]` пуст.

Для сравнения reCAPTCHA (`google.com/recaptcha/api2/demo`) и hCaptcha (`accounts.hcaptcha.com/demo`) создают свои iframe.

## Ожидание

Turnstile вставляет iframe `challenges.cloudflare.com/cdn-cgi/challenge-platform/...` и с тестовым ключом выдаёт токен.

## Заметки

- Причина не исследована: нужно посмотреть, какой шаг `api.js` молча не выполняется (создание iframe, `postMessage`, проверка окружения).
- Скрипт воспроизведения: логика в `.tmp/antibot_probe3.py` (не коммитится) — navigate, `turnstile.render`, ожидание 15 с, чтение iframe/токена.
