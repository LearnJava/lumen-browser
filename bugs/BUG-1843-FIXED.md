# BUG-1843 — устаревшая отрисовка `<input>`: запись значения из оболочки не доходила до трекера DOM-касаний

**Статус:** FIXED 2026-11-26 — каждая запись состояния управляющего элемента из оболочки отмечается в трекере до постановки задания relayout; бокс поля перестраивается.
**Заведён:** 2026-11-26 (ZCode, задача «portal.nextera.ru: падение по «Войти», зависание клика, отрисовка полей, клик по панели»)
**Компонент:** js ([`v8_runtime/runtime.rs`](../crates/js/src/v8_runtime/runtime.rs), [`persistent_js.rs`](../crates/shell/src/persistent_js.rs)), shell ([`lumen/text_input.rs`](../crates/shell/src/lumen/text_input.rs), [`lumen/click.rs`](../crates/shell/src/lumen/click.rs), [`forms.rs`](../crates/shell/src/forms.rs)), dom ([`forms.rs`](../crates/engine/dom/src/forms.rs))

## Симптом

Значение, записанное в поле самим браузером (ввод с клавиатуры, выбор в пикере, переключение checkbox/radio),
оказывается в DOM, но поле отрисовывает прежнее значение. Найдено на странице входа Keycloak портала
`portal.nextera.ru` при вводе логина.

## Причина

Оболочка пишет значение поля в документ из Rust — ни один JS-примитив это не фиксирует. JS-эхо, которое идёт
следом, видит уже стоящее значение и тоже ничего не записывает. Задание инкрементального restyle
(PERF-RELAYOUT-JOB, `85c3dcd50b`) спрашивает у трекера DOM-касаний (`DomTouchedAckReader`), какие боксы
перестраивать, не получает имя поля и переиспользует старый блок.

## Исправление

- `V8JsRuntime::shell_touch_notifier()` возвращает дескриптор, который делает `record_dom_touch` и помечает
  документ устаревшим; вызывается на потоке браузера до постановки задания relayout, без круга через движок.
- `DomTouchedAckReader::touch: Option<ShellControlTouch>` и тип `ShellControlTouch` (`persistent_js.rs`).
- `Lumen::note_shell_control_write(nid)` (`text_input.rs`) — единая точка; вызывается из `write_field_value`
  (ввод текста), `click.rs` (checkbox, radio, выбор цвета и даты в пикерах), `hint_mode.rs`, `spell_menu.rs`.
- `dom::forms::set_radio_checked` и `shell::forms::toggle_radio` возвращают все изменённые узлы — кроме
  нажатого, и члены группы, у которых сняли `checked`.
- Тест `v8_shell_control_touch` ([`dom/tests/v8_shell_control_touch.rs`](../crates/js/src/dom/tests/v8_shell_control_touch.rs)), 2/2.

## Проверка

Настоящим вводом с клавиатуры, 2026-11-25: после правки поле перерисовывает введённое значение.
`cargo clippy` четырёх крейтов (`lumen-dom`, `lumen-js`, `lumen-shell`, `lumen-layout`) с `--all-targets
-- -D warnings` чисто; `scripts/scoped-test.sh` (2026-11-25): 21 825 прошло, 25 провалилось — все 25 совпали
с базой на чистом `main` a0666c3564.

## Остаток

`login_offer.rs` (предложение сохранённого логина) не затронуто. Пиксельную отрисовку поля логина на Keycloak
после этой правки не смотрели: 2026-11-26 пользователь вошёл настоящим вводом без замечаний, снимок экрана
поля не делался.
