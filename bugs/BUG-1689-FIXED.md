# BUG-1689 — `permissions.setPermission` (BiDi / `test_driver.set_permission`) не доходит до запроса разрешения `getUserMedia`

**Статус:** FIXED 2026-10-27 (P6)
**Заведён:** 2026-10-23 (сессия Chat, по вердиктам WPT-RUN-16 — `docs/wpt/pass-rate.md`, строка `mediacapture-streams` таблицы падений)
**Область:** shell (`crates/shell/src/app/automation_cmds.rs` — `SetPermission`; `crates/shell/src/panels/permission_panel.rs`), js (`crates/js/src/notifications_bindings.rs` — `_lumen_ask_permission`)

## Симптом

Тест вызывает `test_driver.set_permission({name: 'microphone'}, 'granted')`, затем
`navigator.mediaDevices.getUserMedia({audio: true})`. После UX-PERMISSIONS срез 4 (`7bbb846ed`)
`getUserMedia` ждёт ответа пользователя в `#permPopover`, решение из BiDi в эту очередь не попадает, и
тест висит до TIMEOUT.

В прогоне 2026-10-22: категория `mediacapture-streams` 8.06 → 5.86, 18 TIMEOUT `GUM-*`,
`MediaStream-*`; «тихий» перепрогон даёт то же (5.86) — это движок, а не нагрузка. Под тем же
механизмом, вероятно, и другие разрешения, которые запрашиваются через `_lumen_ask_permission`
(уведомления, камера, геолокация), — проверить при починке.

## Почему (проверено по коду 2026-10-23)

Два хранилища решений, и между ними нет связи; имя разрешения (`microphone`) совпадает с обеих
сторон.

- BiDi: `permissions_set_permission` (`crates/bidi-server/src/protocol.rs:1565-1595`, диспетчер
  `:716`) → `live.set_permission` (`crates/driver/src/live_session.rs:429`) →
  `AutomationCommand::SetPermission` → `crates/shell/src/app/automation_cmds.rs:437-461`: выполняет
  `_lumen_permission_set` на странице (`crates/js/src/permissions.rs:306-313`, пишет только
  `OVERRIDES[name]`, который читает `navigator.permissions.query()`) и
  `set_global_permission_override` (`crates/js/src/v8_runtime/overrides.rs:146-154`, глобальный список
  name→state без origin). `origin` из команды обязателен, но не используется.
- Запрос: `crates/js/src/media_devices.rs:294-295` → `_lumen_ask_permission('microphone')`
  (`crates/js/src/notifications_bindings.rs:401-407`) → нативный `_lumen_permission_prompt`
  (`:164-176`) → `permission_request_queue` → `crates/shell/src/app/about_to_wait.rs:1090-1112` →
  `PermissionPanel::request(kind)` (`crates/shell/src/panels/permission_panel.rs:242-255`): без
  сохранённого решения кладёт запрос в `pending` и открывает `#permPopover`.
- `_lumen_ask_permission` не смотрит в `OVERRIDES`, а shell смотрит только в
  `PermissionPanel.state_for` (`permission_panel.rs:304`, карта по origin + `lumen_storage`).
  Обработчик `SetPermission` не вызывает `PermissionPanel::set_permission` (`permission_panel.rs:319`).
- Раннер исправен: `tools/wptrunner/wptrunner/executors/executorlumen.py:539-540`, `:675-695` шлёт
  `permissions.setPermission(descriptor, state, origin)`, подставляя `location.origin`.

## Что сделать (направление, не предписание)

В обработчике `SetPermission` (`automation_cmds.rs`) записывать решение и в `PermissionPanel` — по
origin из команды (`PermissionKind::from_page_name`), — так, чтобы оно было временным для сессии
автоматизации и не попадало в постоянное `lumen_storage` пользователя. Альтернатива —
`_lumen_ask_permission` сначала проверяет `OVERRIDES`; выбрать вариант, при котором решение
пользователя и решение драйвера не расходятся между `permissions.query()` и запросом.

## Как проверить

- Тест в `crates/shell` (или BiDi-тест в `crates/bidi-server/src/protocol/tests/commands.rs`):
  после `permissions.setPermission(microphone, granted)` запрос `microphone` с этого origin решается
  без `#permPopover`; `denied` — сразу `NotAllowedError`.
- `tests/wpt/run_corpus.py --prefixes mediacapture-streams` — нет TIMEOUT `GUM-*`/`MediaStream-*` из-за
  ожидания разрешения, балл не ниже 8.06 (2026-10-05).

## Исправление (2026-10-27)

Выбран второй вариант: `permissions.js` отдаёт `_lumen_permission_override(name)` (`granted`/`denied`
из `OVERRIDES`, иначе пусто), а `_lumen_ask_permission` (`notifications_bindings.rs`) сверяется с ним до
постановки запроса в очередь `#permPopover`. Решение драйвера живёт только в `OVERRIDES` и
process-global списке (повторно применяется на следующей навигации), в `PermissionPanel`/`lumen_storage`
пользователя не попадает. `prompt` снимает решение — запрос снова идёт в popover.
Тесты: `permissions::tests::embedder_override_is_visible_to_request_path`,
`notifications_bindings::tests_v8::generic_prompt_honours_embedder_override`.
Перепрогон `mediacapture-streams` не делался (нужен бинарь dev-release).
