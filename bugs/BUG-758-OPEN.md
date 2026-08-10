# BUG-758 — движок не отслеживает transient user activation: `navigator.userActivation` всегда активен

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — литерал `navigator.userActivation`),
потребители: `_lumen_fs_request_error` (там же, Fullscreen §4.3),
`crates/js/src/filesystem_access.rs:2601` (FSA §8.1),
`crates/js/src/local_font_access.rs:151` (Local Font Access §2)
**Найден:** P3, при закрытии [BUG-390](BUG-390-FIXED.md) (2026-08-10)

## Симптом

```
WPT fullscreen/api/document-onfullscreenerror.html
  FAIL Checks that the fullscreenerror event is fired when entering fullscreen fails
       - assert_unreached: Should have rejected: undefined

WPT fullscreen/api/element-request-fullscreen-not-allowed.html
  FAIL requestFullscreen() when not allowed to request fullscreen
       - assert_unreached: Should have rejected: undefined
```

Оба теста вызывают `element.requestFullscreen()` из обычного скрипта — без
клика, без любого другого жеста. Спека (Fullscreen §4.3) требует отказа именно
по отсутствию transient activation. Lumen переходит в fullscreen.

То же самое, в неигровой форме: страница может открыть файловый диалог
(`showOpenFilePicker`) или запросить список локальных шрифтов из `setTimeout`,
хотя обе спеки это прямо запрещают — гейт в обоих модулях написан правильно и
читает `navigator.userActivation`, а тот всегда отвечает «жест был».

## Причина

`navigator.userActivation` — замороженный литерал:

```js
Object.defineProperty(navigator, 'userActivation', {
  value: Object.freeze({ isActive: true, hasBeenActive: true }),
  configurable: true, writable: false, enumerable: true,
});
```

Комментарий рядом объясняет решение: «Single-user interactive desktop app:
always reports the user has activated». Ни в JS-шиме, ни в `v8_runtime.rs`, ни в
шелле нет отметки времени последнего жеста — `grep -rn "activation"
crates/shell/src/*.rs` даёт только `<a>`-«activation behavior» и два
несвязанных комментария. То есть значение не «упрощено», а **отсутствует как
состояние**: любой ответ, кроме «да», сейчас взять неоткуда.

Это делает бесполезной всю группу спековых гейтов «только по жесту
пользователя»: ответ, который всегда «да», не несёт информации (тот же класс,
что [BUG-386](BUG-386-FIXED.md) — `permissions.query()` отвечал `granted` на
любое имя).

## Как чинить

1. Отмечать факт жеста в Rust на входе пользовательских событий (`click`,
   `keydown`, `pointerdown`, `touchend` — HTML LS §6.4 «activation triggering
   input event»): единая точка — путь, которым шелл зовёт
   `_lumen_dispatch_mouse_event` / `_lumen_dispatch_key_event`
   (`crates/js/src/v8_runtime.rs`), там же хранить `last_activation: Instant`.
2. Экспортировать нативную `_lumen_user_activation()` → `{ isActive, hasBeenActive }`
   (`isActive` = прошло меньше окна transient activation, HTML LS даёт 5 с;
   `hasBeenActive` = жест вообще был) и превратить `navigator.userActivation`
   из литерала в аксессор поверх неё.
3. Учесть, что «истребление» активации (consume) требуется спекой при
   срабатывании гейта — как минимум для FSA-диалога.

**Проверка:** WPT `fullscreen/api/document-onfullscreenerror.html` и
`element-request-fullscreen-not-allowed.html` должны стать зелёными без правок в
самом Fullscreen-коде — гейт там уже написан и покрыт юнит-тестом
`request_fullscreen_rejects_without_transient_activation`, который сейчас
подменяет `navigator.userActivation` вручную.

**Осторожно с автоматизацией:** живые прогоны (WPT, MCP/BiDi, graphic tests)
кликают программно; если синтетический клик из автоматизации не будет считаться
жестом, часть существующих сценариев (файловые диалоги в тестах) станет
недостижимой. Решать явно, а не по умолчанию.

## Связанные

* [BUG-390](BUG-390-FIXED.md) — предусловия `requestFullscreen()`; гейт
  активации там реализован, но при текущем ответе `userActivation` мёртв.
* [BUG-386](BUG-386-FIXED.md) — тот же класс: разрешительный ответ вместо
  проверки.
