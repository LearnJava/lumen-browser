# BUG-1198 — `sandbox` без `allow-same-origin` не даёт фрейму непрозрачного происхождения в сообщениях

**Статус:** FIXED 2026-09-28 (P6)
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

## Причина — три дефекта, а не один

Прогон по шагам (проба с `console.log` на каждом шаге в живом окне) показал, что тест висел
не на происхождении, а раньше:

1. **`window.top` фрейма всегда был самим фреймом.** BUG-587 сделал `top` unforgeable-свойством
   окна (`web_api_shim_tail_b.js`), и `installHierarchyAccessors` (`frame_bridge.rs`) молча
   (`try/catch`) не мог его переопределить — `parent` работал, `top` нет. `top.postMessage()`
   из фрейма уходил самому фрейму; в третьем сабтесте он же получал собственное «Hi.» и звал
   `navigation.reload()` — отсюда `TypeError` в логе прогона.
2. **Инлайн-скрипты ребёнка исполнялись до регистрации предков** (известное ограничение
   среза 3 BUG-480): shell ставил слоты `parent`/`top` после `run_scripts_with_dom`.
3. **Происхождение считалось только по URL** (`binding_origin`), флаг песочницы не участвовал,
   идентичности непрозрачного происхождения документа у моста не было; `navigation.reload()`
   отсутствовал, а навигацию из скрипта фрейма shell отбрасывал целиком.

## Фикс

- `top` в полном шиме читает хук `_lumen_frame_top`, который ставит `installHierarchyAccessors`.
- `crates/shell/src/frame_ancestry.rs` — регистрация до первой строки скрипта ребёнка: слоты
  `parent`/`top` в его рантайме и биндинг ребёнка у родителя (без `peer`; после скриптов
  `spawn_frame` регистрирует его ещё раз, с `peer`). Иначе сообщение инлайн-скрипта родитель,
  разбирающий ящик на своём потоке, мог забрать раньше регистрации и доставить с
  `source === null` (у динамических фреймов загрузка идёт на фоновом потоке).
- `FrameDocBinding::opaque_id` — `Some(id)` у документа `sandbox` без `allow-same-origin`;
  `id` из монотонного счётчика (`next_opaque_origin_id`, не адрес документа: адрес
  освобождённого документа может достаться перезагруженному), при повторной регистрации того
  же документа сохраняется (`upsert_binding`). Сообщение такого отправителя —
  `origin === "null"` + `opaque`, по которому `origin_shim.js::_lumen_origin_opaque` отдаёт одну
  запись непрозрачного происхождения на ключ. `targetOrigin` к opaque-адресату — только `'*'`.
- `navigation.reload()` (HTML LS §7.2.9.4) — действие `5` очереди Navigation API. Shell
  применяет его к странице, как и раньше `location.reload()`, а во фрейме — оба
  (`about_to_wait.rs`) — `Lumen::reload_frame`: новый документ того же адреса без записи
  истории, srcdoc перечитывается из атрибута хозяина (`replace_frame_document(idx, None, …)`).

Проверка: WPT `origin-from-messageevent-opaque` 0/3 TIMEOUT → 3/3 OK; каталог
`html/browsers/origin/api` 296/325 → 300/325 (A/B против бинаря того же дня). A/B десяти
фреймовых тестов (`nested-browsing-contexts/window-{top,parent}{,-null}`, `the-iframe-element`,
`sandboxing`) через локальный сервер — регрессий нет, `sandbox-inherit-to-blank-document-
unsandboxed-frame` стал PASS. Юнит-тесты — `crates/js/src/v8_runtime/tests/bug1198_opaque_frame.rs`.

## Остаток

- Слоты `parent`/`top` в реестре ребёнка всегда tuple-origin: сообщение от opaque-sandbox фрейма
  его ВЛОЖЕННОМУ фрейму придёт с origin по URL. У shell нет признака «сам документ-родитель
  непрозрачен» на месте регистрации — это и наследование флагов песочницы вложенным фреймам.
- Собственное происхождение фрейма изнутри — [BUG-1208](BUG-1208-OPEN.md): `window.origin`
  отсутствует целиком, `location.origin` srcdoc-фрейма — `''`.
- Навигация фрейма из его скрипта, кроме перезагрузки и отправки формы (`location.href =`,
  `navigation.navigate()`), по-прежнему отбрасывается.
