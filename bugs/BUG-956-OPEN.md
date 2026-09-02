# BUG-956 — `execCommand('insertText', …)` — тихий no-op вне contenteditable-выделения, ложный `true`

**Статус:** OPEN
**Тип:** дефект реализованного кода — команда `insertText` есть в `_lumen_exec_command` (`dom_core.rs`), но требует уже выставленный `doc.get_selection().anchor`, а `.focus()` его не создаёт.
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 34, живая проба `verify_slice34_gaps.py --variant exec-command-insert-text`)
**Область:** js (`crates/js/src/v8_runtime/install/dom_core.rs` — `_lumen_exec_command`, ветка `"insertText"`; смежно `crates/js/src/shim/web_api_shim_tail_b.js`/`HTMLElement.prototype.focus` — не устанавливает `Selection`)
**Владелец:** P3.

## Симптом

`uievents/textInput/api.html` для каждого из трёх `<input>`/`<textarea>`/
`<div contenteditable>` делает:

```js
el.addEventListener('input', t.step_func(e => { … resolve(); }));
el.focus();
document.execCommand('insertText', false, 'a');
```

и ждёт события `input` через `promise_test`. Ни на одном из трёх элементов
`input` не приходит — `execCommand` возвращает `true` (как будто команда
выполнена), но текст не вставляется вообще: значение/`textContent` остаётся
пустым. Все три подтеста файла висят до TIMEOUT.

## Причина

`_lumen_exec_command` (`dom_core.rs`) для `"insertText"`:

```rust
"insertText" => {
    if let Some(pos) = sel.anchor {
        …
        record_dom_touch_unattributed(&touched);
        dirty.store(true, Ordering::Relaxed);
    }
    true   // <-- возвращается true и когда `if let` не сработал
}
```

`sel` — это `doc.get_selection()`, документная `Selection` (модель Range,
рассчитанная в первую очередь на contenteditable). `.focus()` на `<input>`/
`<textarea>`/`<div contenteditable>` переводит `document.activeElement`
(живая проба это подтверждает — `active-element-is[…] = true` для всех трёх),
но НЕ создаёт `doc.get_selection().anchor` ни для одного из трёх тегов —
у `<input>`/`<textarea>` вообще отдельная модель значения/каретки, не
завязанная на документную `Selection`, а `contenteditable`, судя по пробе,
тоже не получает начального collapsed-выделения при фокусировке.

Из-за этого `if let Some(pos) = sel.anchor` не срабатывает НИ ДЛЯ ОДНОГО из
трёх элементов — блок вставки текста целиком пропускается, `dirty`/`touched`
не выставляются (значит и `input`-событие, которое зависит от этих флагов
через relayout, никогда не всплывает), а функция всё равно возвращает
`true`, будто команда сработала — эта часть отдельно вводит в заблуждение
любой код, который проверяет успех по возвращаемому значению.

## Прямое измерение

Живая проба (`--variant exec-command-insert-text`, dev-release, `main` =
`76c58b60e`): для всех трёх селекторов (`.t1`=input, `.t2`=textarea,
`.t3`=div contenteditable) — `active-element-is[…] = true`,
`exec-command-returned[…] = true`, но `value-after[…]` пустая строка у всех
трёх, и маркер `input-fired[…]` не напечатан ни разу.

## Кого это держит

`uievents/textInput/api.html` — 1 id, все 3 async-подтеста
(`execCommand('insertText', false, 'a')` × `<input>`/`<textarea>`/
`<div contenteditable>`).

## Направление починки

`_lumen_exec_command`/`"insertText"`: если `sel.anchor` пуст, но есть
фокусированный элемент с собственной моделью каретки (`<input>`/
`<textarea>`), вставлять текст через ту же машинерию, что уже использует
`_lumen_contenteditable_insert_text`/форма-специфичный путь (`insert_text_at`
уже параметризован позицией — нужен эквивалент позиции для value-based
контролов), а для `contenteditable` без выделения — сначала создавать
collapsed-selection в начале элемента при `.focus()` (или прямо в этой
ветке, если выделения ещё нет, но `document.activeElement` — потомок
contenteditable-корня). Возврат `true` без реального применения команды —
отдельная правка: `false`/пропуск, если ветка ничего не сделала, а не
безусловный `true`.
