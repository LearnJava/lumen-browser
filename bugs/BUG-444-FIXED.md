# BUG-444 — checkedness не имела хранилища, отдельного от content-атрибута `checked`: `el.checked = …` затирало значение по умолчанию, `defaultChecked`/`form.reset()` восстанавливали его только по снимку

**Статус:** FIXED 2026-08-30
**Компонент:** dom (`crates/engine/dom/src/lib.rs` — `Document::dirty_checkedness` +
`control_checked`/`dirty_checked`/`set_control_checked`/`clear_control_checked`),
js (`crates/js/src/shim/web_api_shim_mid.js` — геттер/сеттер `checked`;
`crates/js/src/shim/web_api_shim_tail_b.js` — `defaultChecked`-рефлексия,
`_lumen_radio_select`, `form.reset()`, `_lumen_gc_collect`;
`crates/js/src/v8_runtime/install/dom_core.rs` — нативы
`_lumen_{get,set,clear}_dirty_checked`), layout (`crates/engine/layout/src/box_tree.rs`
— покраска отметки; `crates/engine/layout/src/style/matching/forms.rs` —
`:checked`/`:indeterminate`), a11y (`crates/engine/a11y/src/lib.rs` —
`checked_state`), shell (`crates/shell/src/forms.rs` — `toggle_checkbox`),
driver (`crates/driver/src/session.rs`, `crates/driver/src/winit_session.rs`
— нативный клик)
**Найден:** P1 при починке [BUG-383](BUG-383-FIXED.md), 2026-07-29
**Починен:** P3, 2026-08-30

## Симптом (до фикса)

```js
var c = document.querySelector('input[type=checkbox][checked]');
c.checked = false;          // снимает галочку
c.defaultChecked            // → должно остаться true
form.reset();               // → должно вернуть галочку
```

Скриптовый путь работал по обходу BUG-383 (снимок `_lumen_default_checked`),
но **щелчок мышью** обходил его целиком: шелл менял content-атрибут `checked`
прямо из Rust, снимок не снимался, значение по умолчанию терялось безвозвратно
— `defaultChecked` и `form.reset()` после первого клика уже ничего не
восстанавливали.

## Причина

HTML LS §4.10.5.5 различает две величины: **checkedness** (текущее состояние,
меняется пользователем и скриптом) и content-атрибут `checked` (значение по
умолчанию, `defaultChecked`). Их связывает «dirty checkedness flag»: после
первого изменения текущего состояния атрибут перестаёт на него влиять.

В Lumen хранилище было одно — сам атрибут. Так было сделано не по недосмотру:
по атрибуту шелл красил чекбокс, `collect_dom_form_fields` собирал форму, а
`element_validity` считал валидность — все три читали документ напрямую из
Rust, и JS-хранилище (как `_input_values` для `value`) их не покрывало.

Тот же дефект модели, что и [BUG-441](BUG-441-FIXED.md) — там про `value`,
здесь про `checked`.

## Фикс

Тем же путём, что BUG-441 (2026-08-04) для `value`: у `Document` заведено
хранилище `dirty_checkedness: HashMap<NodeId, bool>` рядом с `dirty_values`,
где наличие записи и есть dirty checkedness flag; сериализуется, чтобы тиканье
пережило гибернацию таба.

* **`control_checked(id)`** — единственная точка чтения текущей checkedness:
  запись в `dirty_checkedness`, иначе атрибут `checked`. На неё переведены
  `collect_fields_in` (сбор формы), `element_validity` (`required`), покраска
  отметки в `box_tree.rs`, `:checked`/`:indeterminate` в
  `style/matching/forms.rs` и `checked_state` в `lumen-a11y` (AX-дерево читает
  фактическое состояние, а не дефолт).
* **`set_control_checked`/`clear_control_checked`** — единственная точка
  записи/сброса. Content-атрибут не трогается — он остаётся дефолтом, который
  читают `defaultChecked` и восстанавливает `form.reset()`.
* JS-шим: геттер/сеттер `checked` на `HTMLInputElement.prototype` переведены на
  новые нативы `_lumen_{get,set,clear}_dirty_checked`; `defaultChecked` стал
  обычной табличной IDL-рефлексией атрибута `checked` (`_lumen_install_reflection`)
  — весь обход `_lumen_default_checked`/`_lumen_capture_default_checked` снят;
  `_lumen_radio_select` (радио-группа) и `HTMLFormElement.prototype.reset`
  переведены на те же нативы.
* Нативный клик (`session.rs::InProcessSession`, `winit_session.rs::WinitSession`,
  `shell/forms.rs::toggle_checkbox`) теперь читает/пишет через
  `control_checked`/`set_control_checked` вместо прямой правки атрибута —
  это и был путь, который обход BUG-383 не мог закрыть в принципе.

Гейт — `crates/driver/tests/cases/idl_reflection.rs`: два новых теста,
`native_click_keeps_default_checkedness` (клик не портит `defaultChecked`,
`form.reset()` после клика восстанавливает) и
`native_and_scripted_checkedness_share_one_storage` (клик и скрипт видят одно
и то же состояние). Плюс юнит-тесты `Document::control_checked` и читателей в
`lumen-dom`, `lumen-layout`, `lumen-a11y`.

## Связанные

* [BUG-441](BUG-441-FIXED.md) — тот же дефект модели для `value`, починен тем
  же приёмом раньше.
* [BUG-383](BUG-383-FIXED.md) — правка, которая вскрыла дефект и поставила
  временный обход (`_lumen_default_checked`), снятый этим фиксом.
