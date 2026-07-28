# BUG-373 — все 9 `new DOMException(...)` в `filesystem_access.rs` переданы аргументами наоборот: `.name` получает человеческий текст, `.message` — имя ошибки, поэтому каждое отклонение File System Access непроверяемо (`e.name === 'AbortError'` всегда false)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/filesystem_access.rs` — строки 561, 628, 630, 643, 645, 653, 667, 679, 693)
**Найден:** P2, WPT-VENDOR-file-system-access (2026-07-28), проба `--dump-layout` вне WPT (`.tmp/fsa-probe.html`)

## Симптом

Конструктор `DOMException` в движке — стандартного порядка `(message, name)`;
подтверждено пробой на живой странице:

```
DE.ctor order = name=TheName msg=the message      // new DOMException('the message','TheName')
```

Тот же порядок используется везде в `dom.rs` (≈20 мест, например
`new DOMException('DOM node limit exceeded', 'QuotaExceededError')` —
`dom.rs:4773`) и во всех остальных модулях-шимах (`bluetooth.rs:133`,
`media_capabilities.rs:83`, `payment_request.rs:121` и т.д.).

`filesystem_access.rs` — единственный модуль, который во **всех девяти** местах
передаёт их наоборот, именем вперёд:

| Строка | Как написано | Что получается |
|---|---|---|
| 561 | `new DOMException('NotAllowedError', 'Write permission denied or user cancelled')` | `name='Write permission denied or user cancelled'` |
| 628 | `new DOMException('NotSupportedError', 'create not supported in Phase 1')` | `name='create not supported in Phase 1'` |
| 630 | `new DOMException('NotFoundError', 'File not found: ' + name)` | `name='File not found: …'` |
| 643 | `new DOMException('NotSupportedError', 'create not supported in Phase 1')` | `name='create not supported in Phase 1'` |
| 645 | `new DOMException('NotFoundError', 'Directory not found: ' + name)` | `name='Directory not found: …'` |
| 653 | `new DOMException('NotSupportedError', 'removeEntry not supported in Phase 1')` | `name='removeEntry not supported in Phase 1'` |
| 667 | `new DOMException('AbortError', 'The user aborted a request.')` | `name='The user aborted a request.'` |
| 679 | `new DOMException('AbortError', 'The user aborted a request.')` | `name='The user aborted a request.'` |
| 693 | `new DOMException('AbortError', 'The user aborted a request.')` | `name='The user aborted a request.'` |

Живая проверка двух путей, которые не открывают OS-диалог и потому проверяемы
headless:

```
ERR.dir.removeEntry rejected   = name=removeEntry not supported in Phase 1  msg=NotSupportedError  isDOMException=true
ERR.dir.getFileHandle rejected = name=File not found: nope.txt              msg=NotFoundError
```

`instanceof DOMException` при этом истинно — объект правильного типа, перепутаны
только поля.

## Почему это не косметика

Единственный предписанный спекой способ различить причину отказа — `e.name`.
Весь идиоматический код вида

```js
try { await showOpenFilePicker(); }
catch (e) { if (e.name === 'AbortError') return; throw e; }
```

сейчас не срабатывает: `e.name` равно `'The user aborted a request.'`. Отмена
пользователем неотличима от настоящей ошибки, и приложение уходит в ветку
обработки сбоя на каждом закрытии диалога.

Симметрично: `e.message` содержит `'AbortError'` вместо описания, так что
диагностика в консоли тоже вводит в заблуждение.

## Измерение в WPT

Вендоренный `tests/wpt/file-system-access/showPicker-errors.https.window.js`
целиком построен на `promise_rejects_dom(t, 'SecurityError', …)` /
`promise_rejects_js(t, TypeError, …)` — все 37 его сабтестов сверяют
именно `name`. Из них 3 первых (`Showing a picker requires user activation`)
провалятся дважды: и по перепутанному имени, и потому что проверки
пользовательской активации нет вовсе (см. [[BUG-374]]).

В прогоне 2026-07-28 id `/file-system-access/showPicker-errors.https.window.html`
дал TIMEOUT по HTTPS-порт-гэпу — измерение получено пробой.

## Ожидается

Поменять аргументы местами во всех девяти местах:
`new DOMException('<человеческий текст>', '<ИмяОшибки>')`.

Правка механическая и не затрагивает V8/rquickjs по отдельности — `FSAL_SHIM`
общий для обоих движков (`filesystem_access.rs:404` и `:488`).

Проверять фикс следует по `e.name` на отклонённом промисе, а не по факту
отклонения: текущие 34 юнит-теста `filesystem_access::tests` зелены и этот баг
не видят, потому что ни один из них не смотрит внутрь ошибки — они проверяют
`typeof …then === 'function'` (см. [[feedback_green_test_can_mask_broken_feature]];
тот же способ маскировки, что у BUG-365 в `eye_dropper::tests`).

## Заметки

- Проба и вывод целиком: `.tmp/fsa-probe.html`, `.tmp/fsa-probe.log`.
- Стоит грепнуть остальные шимы на тот же перевёрнутый порядок — здесь он
  единообразен по всему файлу, что похоже на однократную ошибку автора модуля,
  но проверка дешёвая.
