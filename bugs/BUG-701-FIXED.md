# BUG-701 — `document.write()`/`document.writeln()` отсутствовали целиком: `TypeError: document.write is not a function`

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs` — `document` singleton в `WEB_API_SHIM`)
**Найден:** живая проверка `https://www.tbank.ru/auth/login/` (запрос пользователя — открыть браузер, зайти в личный кабинет), не через WPT — cdnhealth-скрипт страницы вызывает `document.write()` безусловно, вызов бросал `TypeError` и обрывал остаток исполняемого `<script>`-блока

## Фикс (2026-08-09)

`document.write`/`document.writeln` не были реализованы вовсе (`grep -n 'document\.write'
crates/js/src/dom.rs` — ноль хитов до фикса). Спека (HTML LS §8.4.4) на закрытом
документе (не во время активного парсинга) требует implicit `document.open()`,
который **стирает весь документ** — для гидратируемого SPA-корня (ровно случай
tbank.ru) это было бы худшей регрессией, чем сам `TypeError`. Вместо полной спековой
семантики (нужен реальный active-parser insertion point, которого шим не отслеживает):

- пока `document.readyState === 'loading'` — текст вставляется в конец `<body>`
  через `insertAdjacentHTML('beforeend', ...)`, что приближённо соответствует месту
  вставки синхронного inline-`<script>` во время парсинга;
- после `'complete'`/`'interactive'` — no-op, как document.write()-intervention в
  реальных браузерах для скриптов, вызывающих его после загрузки, вместо разрушения
  страницы.

`writeln` — то же самое плюс `\n` после каждого аргумента.

3 новых юнит-теста в `dom::tests` (`document_write_inserts_while_loading`,
`document_writeln_appends_newline`, `document_write_is_noop_after_complete`),
`cargo test -p lumen-js --features v8-backend document_write` — 3/3 зелёных.
Живая проверка `--mcp-live-port`: `typeof document.write === 'function'`,
вставленный `<span>` находится по `getElementById`.

**Не покрыто:** точное соответствие спеке (multiple-document `open()`/`close()`
цикл, `ignore-destructive-writes-counter`, поведение внутри вложенного парсера) —
не нужно ни одному известному сейчас реальному кейсу; при появлении WPT-сигнала
по `document.write`/`document.open` смотреть отдельно.
