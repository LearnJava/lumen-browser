# BUG-1811 — `script.addPreloadScript` принимается, но не исполняется

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-11-15 (P3, попутно срез 1 [BUG-1647](BUG-1647-OPEN.md))
**Область:** driver (`crates/bidi-server/src/protocol.rs::script_add_preload`, `crates/driver/src/types.rs`, `crates/shell/src/app/automation_cmds.rs`, `crates/js/src/v8_runtime/overrides.rs`, `crates/js/src/v8_runtime.rs::install_dom`)

## Симптом

`script.addPreloadScript` отвечает успехом и выдаёт id, `PreloadScript` кладётся в
`state.preload_scripts`, но ни один документ его не исполняет.

Проба `.tmp/pm_cross.py`: `script.addPreloadScript` с
`{"functionDeclaration":"() => { window.__pre = (window.__pre || 0) + 1; }"}` →
`browsingContext.navigate` → в top-realm `window.__pre === undefined` (2/2 прогона).
То же с логгером `message`/`postMessage` из среза 1 BUG-1647: `window.__pl` пуст, из-за чего
handshake сторонних бандлов (reCAPTCHA anchor, Cloudflare Turnstile) нельзя снять до первого
скрипта страницы.

## Причина

Команда была учётной записью в BiDi-состоянии: ни `LiveWindowSession`, ни
`AutomationCommand` не несли preload-скрипт в шелл, а `install_dom` ничего о нём не знал.
Единственный процесс-глобальный канал «настройка → следующий документ» (`overrides.rs`,
BUG-295) preload-скриптов не имел.

## Починено (срез 1, 2026-11-15, P3)

Скрипт идёт тем же путём, что оверрайды BUG-295 — «процесс-глобальная настройка, которую
читает `install_dom` при создании документа»:

- `crates/js/src/v8_runtime/overrides.rs`: `GLOBAL_PRELOAD_SCRIPTS` +
  `set_global_preload_script` / `remove_global_preload_script` /
  `global_preload_scripts` (готовые к исполнению обёртки
  `(function(){ var f = (<functionDeclaration>); if (typeof f === 'function') f(); })()`).
- `install_dom` исполняет их **последним шагом перед** `seal_internal_globals_v8` — после
  всех шимов (скрипт может пользоваться любым API), до первой строки скриптов страницы и в
  каждом кадре (кадр идёт через тот же `install_dom`). Битый скрипт печатается в stderr и
  бутстрап не рвёт.
- `AutomationCommand::{AddPreloadScript{id, function_declaration}, RemovePreloadScript(id)}` +
  `LiveWindowSession::{add_preload_script, remove_preload_script}`; шелл пишет/снимает запись
  в реестре, id помнит BiDi-состояние.
- `script.addPreloadScript` / `script.removePreloadScript` доходят до живого окна (при
  отсутствии окна поведение прежнее — учёт в состоянии).

**Ограничения среза:** BiDi-`channel`-аргументы не поддержаны (вызов без аргументов), а
`contexts` — только книжный учёт: живое окно одно, id BiDi-контекстов не отображаются на его
вкладки, поэтому скрипт с `contexts` применяется ко всему окну (`shortcut:`-комментарий в
`protocol.rs`).

## Проверка

- `cargo test -p lumen-js --features v8-backend --lib -- bug1811` — 1/1: скрипт исполняется в
  первом и втором документе, после снятия — нет; битая запись бутстрап не ломает.
- `cargo test -p lumen-bidi-server` — 125/125, включая
  `add_and_remove_preload_script_reach_the_live_window` (команда доходит до живого канала).
- Проба `.tmp/bug1811_probe.py` (живое окно `--bidi-port`, `LUMEN_NO_ADBLOCK=1`, 2/2):
  top-realm `pre=1` и кросс-оригин кадр `pre=1` (кадр сообщает о себе сам; same-origin кадр
  сообщение в родителя не доставляет — семья [BUG-480](BUG-480-OPEN.md), к preload отношения
  не имеет).

## Связи

- Дорогу к этому багу проложил срез 1 [BUG-1647](BUG-1647-OPEN.md): без preload-логгера
  handshake `api.js ↔ anchor` не снять. Следующий срез BUG-1647 начинается с логгера, который
  теперь работает.
