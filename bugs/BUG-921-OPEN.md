# BUG-921 — имя вложенного browsing context читается из атрибута `name` хоста на каждое обращение, а не задаётся один раз при создании

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, прогоном категории WPT попутно к [BUG-854](BUG-854-FIXED.md))
**Область:** `crates/js/src/frame_bridge.rs` (геттер `window.name` в контексте
ребёнка — `_lumen_f_attr(p, host, 'name')` на каждое чтение)
**Владелец:** P1/P3 (дорожка [BUG-480](BUG-480-OPEN.md))

## Симптом

```html
<iframe src="child.html" name="initialvalue"></iframe>
<script>
  // ребёнок сообщает своё window.name родителю
  frame.setAttribute("name", "meh");   // спека: имя контекста НЕ меняется
  // ребёнок сообщает снова → Lumen отдаёт "meh", ожидается "initialvalue"
  frame.removeAttribute("name");       // спека: имя контекста НЕ меняется
</script>
```

HTML LS: значение атрибута `name` используется для именования вложенного
navigable **в момент его создания**; дальше имя — свойство самого контекста, и
меняет его только присваивание `window.name` изнутри (или новая навигация).
Lumen же держит имя не в контексте, а в атрибуте: геттер `window.name` ребёнка
на каждое чтение спрашивает `name` у узла-хоста в документе родителя, поэтому
любая правка атрибута задним числом переименовывает уже существующий контекст.

## Прямое измерение

`run_report.py --all --root html/browsers/windows/nested-browsing-contexts
--recursive` (dev-release, Windows, `main` = `91cc86628`) — 6 подтестов
`name-attribute.window.js`, по три на каждый тег:

```
FAIL same-origin <frame>                - assert_equals: expected "" but got "meh"
FAIL same-origin <frame name=>          - assert_equals: expected "" but got "meh"
FAIL same-origin <frame name=initialvalue> - assert_equals: expected "initialvalue" but got "meh"
FAIL same-origin <iframe>               - assert_equals: expected "" but got "meh"
FAIL same-origin <iframe name=>         - assert_equals: expected "" but got "meh"
FAIL same-origin <iframe name=initialvalue> - assert_equals: expected "initialvalue" but got "meh"
```

Оба тега ведут себя одинаково — дефект не про `<frame>` и не про `<iframe>`, а
про модель хранения имени. Три `<frame>`-подтеста стали видны только что: до
[BUG-854](BUG-854-FIXED.md) фрейм не грузился вовсе, и они были TIMEOUT.

Заметьте, что обмен сообщениями ребёнок → родитель в этих подтестах работает
(иначе ассерт не сработал бы), то есть найден именно шаг после доставки.

## Что нужно

Имя должно жить в биндинге контекста: `register_frame_document` уже получает
`name` хоста — его и надо запомнить как имя контекста один раз, а
`window.name` ребёнка читать из этого слота (и писать в него из сеттера,
который сейчас держит `__customName` отдельно). Атрибут после этого влияет
только на `window[name]` у родителя и на следующую навигацию фрейма.

Осторожно: у родителя именованный доступ `window[name]` спека тоже определяет
через *имя контекста*, а не через атрибут, так что оба доступа должны читать
один слот, иначе они разойдутся.

## Как проверить фикс

`run_report.py --all --root html/browsers/windows/nested-browsing-contexts
--recursive` — шесть перечисленных подтестов должны стать PASS (остальные
`cross-origin`-варианты той же шестёрки блокируются отдельно: алиасы
`www1.127.0.0.1` режутся как mixed content, `WPT-RUN-10`).
