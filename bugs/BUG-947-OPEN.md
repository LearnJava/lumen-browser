# BUG-947 — `new Notification()` под запретом молча ничего не делает вместо `error`

**Статус:** OPEN
**Тип:** дефект реализованного кода — ветка обработки написана и намеренно упрощена.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31)
**Область:** js (`crates/js/src/notifications_bindings.rs` — конструктор `Notification`, комментарий на строке 116)
**Владелец:** P3.

## Симптом

Собственный doc-комментарий `notifications_bindings.rs:116` прямо говорит:
конструктор «Does nothing (silent drop) when permission is 'denied'».
Разрешение по умолчанию — `'denied'` (privacy-first,
`notifications_bindings.rs:12`/`:53`), а HTML Notifications API §6 шаг 5.1
требует в этом случае поставить в очередь задачу, которая диспатчит
`error` на созданный объект. Разница между «ничего не произошло» и
«обещание, которое тест ждёт, settled» — ровно то, что вешает файл: тест
навешивает обработчик на `onerror` и ждёт его вызова, а его никогда не
зовут.

## Прямое измерение

`grep -n "silent\|denied" crates/js/src/notifications_bindings.rs`:
```
116:   * Does nothing (silent drop) when permission is 'denied'.
```
Комментарий описывает уже реализованное (упрощённое) поведение, не догадку.

## Кого это держит

`notifications/constructor-non-secure.html` — конструирует `Notification`
без разрешения и ждёт `onerror`; обработчик не срабатывает никогда.

## Направление починки

В той же ветке, где сейчас `return` (silent drop) при `permission ===
"denied"`, поставить в очередь задачу (не синхронно — тот же принцип, что
у любого другого колбэка, который движок делает от имени страницы, см.
гочу CLAUDE.md про queue-not-inline), диспатчащую `error` на объект
`Notification`. Небольшая точечная правка одной ветки, не новая
подсистема.
