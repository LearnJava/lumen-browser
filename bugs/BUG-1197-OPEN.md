# BUG-1197 — `sandbox` без `allow-same-origin` не даёт фрейму непрозрачного происхождения в сообщениях

**Статус:** OPEN
**Заведён:** 2026-09-27 (P1, при закрытии [BUG-585](BUG-585-FIXED.md) / GAP-ORIGIN)
**Область:** js (`crates/js/src/frame_bridge.rs:507` — `binding_origin`;
`crates/js/src/shim/web_api_shim_mid_b4.js:2162` — `_lumen_deliver_frame_message`), нет
`navigation.reload()`

## Симптом

WPT `html/browsers/origin/api/origin-from-messageevent-opaque.window.html`: все 3 сабтеста
TIMEOUT. Тест вставляет `<iframe sandbox="allow-scripts" srcdoc="…window.top.postMessage(…)">`
и ждёт в родителе `message` с `e.source === el.contentWindow`, где `Origin.from(e).opaque`.

Что установлено по коду (не прогоном по шагам):

- `event.origin` кросс-фреймового сообщения вычисляет `binding_origin(url, fallback)` только по
  URL документа-отправителя (`about:*` → origin получателя). Флаг песочницы в нём не участвует,
  поэтому сообщение из песочницы без `allow-same-origin` приходит с tuple-origin родителя, а не
  с непрозрачным (HTML LS §7.1.1, «sandboxed origin browsing context flag»). `Origin.from(e)`
  повторяет это `.origin` (GAP-ORIGIN регистрирует его как настоящее происхождение события).
- По спеке непрозрачное происхождение одного документа должно быть одним и тем же для всех его
  сообщений и новым после перезагрузки — сейчас идентичности непрозрачного происхождения
  документа у моста нет вовсе.
- Третий сабтест вызывает `navigation.reload()` — в логе прогона
  `Uncaught TypeError: navigation.reload is not a function`.

Почему тест именно висит (не доставлено сообщение или не совпало `e.source`), не проверено.

## Что сделать

Пронести в биндинг фрейма признак непрозрачного происхождения (sandbox без
`allow-same-origin`) и идентификатор документа; `_lumen_deliver_frame_message` отдавать
`origin === 'null'` и регистрировать для `Origin.from` одно непрозрачное происхождение на
документ-отправитель. Отдельно — `NavigationHistory`/`navigation.reload()`.
