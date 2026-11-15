# BUG-1811 — `script.addPreloadScript` принимается, но не исполняется

**Статус:** OPEN
**Заведён:** 2026-11-15 (P3, попутно срез 1 [BUG-1647](BUG-1647-OPEN.md))
**Область:** driver (`crates/bidi-server/src/protocol.rs::script_add_preload`)

## Симптом

`script.addPreloadScript` отвечает успехом и выдаёт id, `PreloadScript` кладётся в
`state.preload_scripts`, но ни один документ его не исполняет.

Проба `.tmp/pm_cross.py`: `script.addPreloadScript` с
`{"functionDeclaration":"() => { window.__pre = (window.__pre || 0) + 1; }"}` →
`browsingContext.navigate` → в top-realm `window.__pre === undefined` (2/2 прогона).
То же с логгером `message`/`postMessage` из среза 1 BUG-1647: `window.__pl` пуст, из-за чего
handshake сторонних бандлов (reCAPTCHA anchor, Cloudflare Turnstile) нельзя снять до первого
скрипта страницы.

## Что сделать

Прокинуть `state.preload_scripts` в момент создания realm'а нового документа (там же, где
устанавливается шим ОС) — с учётом `contexts`-фильтра, семантики `sandbox`
(`script.addPreloadScript.sandbox`) и порядка регистрации.

## Проверка

- Проба выше: после `navigate` `window.__pre >= 1` в top-realm и в кросс-ориентирном кадре.
- `crates/bidi-server` — тест на «зарегистрирован → исполнен в новом документе».

## Связи

- Дорогу к этому багу проложил срез 1 [BUG-1647](BUG-1647-OPEN.md): без preload-логгера
  handshake `api.js ↔ anchor` не снять.
