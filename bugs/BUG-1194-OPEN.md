# BUG-1194 — `input.performActions`: `pointerMove` без клика не наводит курсор, `keyDown('\uE00C')` не Escape

**Статус:** OPEN
**Заведён:** 2026-09-26 (P1, по ходу GAP-INTERESTINVOKER — вне выданного пункта).
**Область:** WPT-исполнитель / BiDi —
[`crates/bidi-server/src/protocol.rs`](../crates/bidi-server/src/protocol.rs) `replay_input_actions`,
[`crates/shell/src/app/about_to_wait.rs`](../crates/shell/src/app/about_to_wait.rs) `AutomationCommand::Type`.

## Симптом (по коду)

`replay_input_actions` моделирует только «клик и набор текста»:

- `pointerMove {x,y}` лишь запоминает точку (`last_point`) и ничего не отправляет в окно. Без
  следующего `pointerDown` курсор не двигается: `hovered_nid` не меняется, `mouseover`/`mouseout`/
  `pointerover`/`:hover` не возникают. Путь `WindowEvent::CursorMoved`
  (`crates/shell/src/app/window_event/cursor_moved.rs`) с этими событиями из автоматизации не
  вызывается.
- `keyDown` источника `key` склеивается в строку и уходит в `live.type_text(last_point, text)` →
  `AutomationCommand::Type`, которая **сначала кликает** в `last_point` (при отсутствии указателя —
  `(0,0)`) и печатает символы. Коды WebDriver из области PUA (`\uE00C` Escape, `\uE004` Tab,
  `\uE008` Shift) не переводятся в клавиши: `keydown` с `key === 'Escape'` не приходит, а
  клик ещё и сдвигает фокус.

## Что это ломает

Все тесты на `test_driver.Actions().pointerMove(...).send()` без клика и на
`keyDown('\uE00C')`. В `html/semantics/interestfor` (GAP-INTERESTINVOKER) это все hover-варианты
(`hoverOver()` из `resources/invoker-utils.js`) и все проверки «клавиши потери интереса»
(`sendLoseInterestHotkey()`). Движок в этих местах сам по себе работает: на реальный
`mouseover` и на `keydown` Escape шим реагирует (юнит-тесты
`crates/js/src/dom/tests/v8_details_dialog_popover.rs`, `escape_loses_interest_without_cancel`).
Часть hover-вариантов вдобавок умирает раньше — на селекторе `:root > *|body:nth-child(2)` для
элемента без `id` ([BUG-1063](BUG-1063-FIXED.md)).

## Что сделать

- `pointerMove` → команда перемещения курсора в окно, которая проходит тот же путь, что
  `CursorMoved` (hit-test, смена `hovered_nid`, `pointerout/mouseout/.../mouseover/pointerenter`).
- Ключи WebDriver PUA (`\uE000`–`\uE05D`) → `KeyDown/KeyUp` с соответствующим `key`/`code`
  вместо печати символа; `keyDown`/`keyUp` без указателя не должны кликать.
