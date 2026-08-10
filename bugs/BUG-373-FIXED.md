# BUG-373 — все 9 `new DOMException(...)` в `filesystem_access.rs` переданы аргументами наоборот: `.name` получает человеческий текст, `.message` — имя ошибки, поэтому каждое отклонение File System Access непроверяемо (`e.name === 'AbortError'` всегда false)

**Статус:** FIXED 2026-08-10 (P3, ветка `p3-bug-373`)
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

Правка механическая. На момент подачи бага (2026-07-28) `FSAL_SHIM`
устанавливался на обоих движках; rquickjs-twin снесён позже, в S12b-B20
(2026-08-04) — сейчас установка единственная, на V8 (`filesystem_access.rs:416`)
— актуализировано P1 2026-08-04, P3-v8-post-audit.

Проверять фикс следует по `e.name` на отклонённом промисе, а не по факту
отклонения: текущие 34 юнит-теста `filesystem_access::tests` зелены и этот баг
не видят, потому что ни один из них не смотрит внутрь ошибки — они проверяют
`typeof …then === 'function'` (см. [[feedback_green_test_can_mask_broken_feature]];
тот же способ маскировки, что у BUG-365 в `eye_dropper::tests`).

## Исправлено (P3, 2026-08-10)

Из девяти мест к моменту фикса оставалось четыре: перезапись модуля под
[BUG-372](BUG-372-FIXED.md) убрала пять заглушечных `throw` из
`getFileHandle`/`getDirectoryHandle`/`removeEntry` вместе с их текстами, а
пришедший им на смену хелпер `fsThrow(name, message)` уже строил
`new DOMException(message, name)` правильно. Оставшиеся четыре — три пикера
(`showOpenFilePicker`/`showSaveFilePicker`/`showDirectoryPicker`,
`AbortError`) и `createWritable()` при отказе в записи (`NotAllowedError`) —
переставлены в порядок `(message, name)`.

Проверка обратного порядка по всему крейту (`new DOMException('<Имя>Error'`,
включая многострочные вызовы) больше не даёт ни одного попадания в
`filesystem_access.rs`. Единственное соседнее совпадение —
`eye_dropper.rs:30/36/52`, где оба аргумента равны `'AbortError'`, поэтому
`name` там верен, а неточен только текст `message` (в скоуп не входит,
относится к BUG-365).

**Пять новых тестов** (`filesystem_access::tests_v8`) впервые смотрят внутрь
отклонения — `"<instanceof DOMException>|<name>|<message>"` целиком:
`cancelled_{open,save,directory}_picker_rejects_with_abort_error`,
`refused_write_permission_rejects_with_not_allowed_error` и
`missing_entry_rejects_with_not_found_error` (последний закрепляет уже
корректный путь `fsThrow`, чтобы два способа поднять исключение в модуле не
разъехались). A/B на возвращённом дефекте: четыре из пяти краснеют ровно
перестановкой (`true|The user aborted a request.|AbortError` вместо
`true|AbortError|The user aborted a request.`), пятый остаётся зелёным.

Побочно вскрылось, почему 34 старых теста не могли увидеть баг даже при
желании: `DOMException` в V8 появляется только из `install_dom`
(`v8_runtime.rs`, `DOM_EXCEPTION_POLYFILL`), а тестовая обвязка модуля его не
зовёт — до этой правки любой `throw new DOMException(...)` в шиме превращался
в `ReferenceError`, и отклонение вообще не было `DOMException`. Полифилл
поднят до `pub(crate)`, обвязка `with_fsa_for` теперь исполняет его сразу
после DOM-заглушек: тест сверяется с настоящим конструктором движка, а не с
собственноручно написанным двойником — иначе утверждение о порядке аргументов
доказывало бы само себя.

`cargo test -p lumen-js --features v8-backend --lib filesystem_access` —
52/52, `cargo clippy -p lumen-js --all-targets --features v8-backend` чист.

## Заметки

- Проба и вывод целиком: `.tmp/fsa-probe.html`, `.tmp/fsa-probe.log`.
- Стоит грепнуть остальные шимы на тот же перевёрнутый порядок — здесь он
  единообразен по всему файлу, что похоже на однократную ошибку автора модуля,
  но проверка дешёвая. — Сделано, см. выше.
