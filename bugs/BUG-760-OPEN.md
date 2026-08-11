# BUG-760 — headless MCP `navigate` не разбирает `file://` URL: теряется буква диска

**Статус:** OPEN
**Компонент:** driver (`crates/driver/src/session.rs:790` —
`InProcessSession::navigate`, ветка `url.strip_prefix("file://")`)
**Найден:** P3, 2026-08-11, при построении живой пробы к
[BUG-393](BUG-393-FIXED.md)

## Симптом

Headless-запуск `lumen.exe --mcp-port N about:blank`, затем MCP-инструмент
`navigate` с обычным для остальных проб `file://`-URL:

```
navigate {"url": "file:///D:/RustProjects/lumen-browser/.tmp/probe393.html"}
→ -32603 Navigate error: io error: не удалось прочитать
  /D:/RustProjects/lumen-browser/.tmp/probe393.html:
  Синтаксическая ошибка в имени файла… (os error 123)
```

Тот же URL через **живое** окно (`--mcp-live-port`) грузится нормально —
см. `.tmp/probe388.py`, где `file:///` + `navigate` работает.

Обход: передавать headless-`navigate` голый путь без схемы
(`D:/RustProjects/.../probe393.html`) — ветка «прямой файловый путь без
схемы» в конце той же функции срабатывает корректно.

## Причина

`crates/driver/src/session.rs`, `impl BrowserSession for InProcessSession`:

```rust
if let Some(path) = url.strip_prefix("file://") {
    let bytes = std::fs::read(path)…
```

Наивный `strip_prefix("file://")` от `file:///D:/…` оставляет `/D:/…` —
ведущий слэш перед буквой диска не снимается, и Windows отвергает путь
(`ERROR_INVALID_NAME`). Обработки буквы диска, в отличие от
`page_source_for_automation_url` в шелле, здесь нет вовсе.

## Шире, чем `file://` (P3, 2026-08-11, проба к [BUG-394](BUG-394-FIXED.md))

Та же функция не знает и про `about:`-схему: headless-`navigate`
с `{"url": "about:blank"}` падает

```
-32603 Navigate error: io error: не удалось прочитать about:blank:
  не удается найти указанный файл. (os error 2)
```

— строка проваливается мимо всех веток со схемой в конечную ветку
«голый путь» и уходит в `File::open("about:blank")`. В живом окне
(`--mcp-live-port`) `about:blank` — штатный стартовый документ, поэтому
рецепт «стартовать с `about:blank`, дальше `navigate`» из CLAUDE.md
(обход [BUG-651](BUG-651-OPEN.md)) в headless-режиме не работает вовсе:
пробе приходится навигировать на файл голым путём. Чинить надо тем же
одним разборщиком URL, что и `file://`, — список схем у него уже есть.

## Как чинить

Разбирать `file://`-URL тем же кодом, что уже умеет это делать
(`page_source_for_automation_url`, `crates/shell/src/main.rs`), а не
собственным `strip_prefix` — то есть вынести разбор в общее место
(`lumen-core`?) и звать из обоих. Иначе третий потребитель повторит ту же
ошибку в четвёртый раз.

Регрессия: unit-тест на `InProcessSession::navigate` с
`file:///<abs-win-path>` и с `file:///<abs-unix-path>`.

## Связанные

* [[BUG-651]] — та же ошибка в `PageSource::from_arg` (начальный CLI-аргумент):
  схема не снимается вообще. Общая причина семейства — разбор `file://`
  скопирован по местам вместо одного входа.
* [[BUG-723]] — потеря двоеточия диска в `_url_resolve` шима на `file://`-странице.
* [[BUG-438]] — провалившаяся навигация в live-окне отвечает успехом; здесь,
  в отличие от неё, headless-путь честно возвращает ошибку.
