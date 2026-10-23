# BUG-1690 — у `iframe.contentWindow.document` нет `open`/`write`/`writeln`/`close`

**Статус:** OPEN
**Заведён:** 2026-10-23 (сессия Chat, по вердиктам WPT-RUN-16 — `docs/wpt/pass-rate.md`, строка `quirks` таблицы падений)
**Область:** js (`crates/js/src/frame_bridge.rs` — `docFacade`)

## Симптом

`iframe.contentWindow.document.open()` бросает `TypeError: document.open is not a function`; так же
`write`, `writeln`, `close`. В главном документе они есть.

В прогоне 2026-10-22 так падают **11** тестов `tests/wpt/quirks/` (ERROR; было и на 2026-10-05).
Кандидаты по grep (13):

- через `tests/wpt/quirks/support/test-ref-iframe.js:11` (`win.document.open()`) —
  `hashless-hex-color/{quirks,limited-quirks,no-quirks}.html`,
  `unitless-length/{quirks,limited-quirks,no-quirks}.html`;
- `q.document.open()` у `contentWindow` — `blocks-ignore-line-height.html:23`,
  `line-height-calculation.html:28`, `percentage-height-calculation.html:34`,
  `table-cell-nowrap-minimum-width-calculation.html:28`, `table-cell-width-calculation.html:28`;
- `class-names-across-iframes.html:23`, `active-and-hover-manual.html:103` (ручной, вероятно не
  гоняется).

Тот же приём (`frame.contentWindow.document.open(); write(...); close()`) широко встречается и вне
`quirks`, так что прирост вероятен и в других категориях.

## Почему (проверено по коду 2026-10-23)

- Документ фрейма для родителя — JS-фасад `docFacade(bid)` (`crates/js/src/frame_bridge.rs:2214`,
  `var d = {}`), его отдаёт `winFacade().document` (`:2283`). Геттеры: `body`, `head`,
  `documentElement`, `title`, `URL`, `documentURI`, `contentType`, `referrer`, `readyState` (всегда
  `'complete'`), `defaultView`, `location`; методы: `getElementById`, `querySelector(All)`,
  `createElement`, `createTextNode` — через нативы `_lumen_f_*`. `open`/`write`/`writeln`/`close` нет.
- В главном документе они в `crates/js/src/shim/web_api_shim_mid.js:14329` (`write`), `:14332`
  (`writeln`), `:14345` (`open`), `:14355` (`close`) и работают на глобалах главного документа
  (`_doc_ready_state`, `_doc_explicit_open`, `_lumen_document_write`) — делегировать им фасад не может.
- Пробел перечислен среди «не входит» в зонтичном [BUG-480](BUG-480-OPEN.md) (ДОРАБОТКА → FRAME);
  этот баг — узкий срез оттуда, после починки отметить его в BUG-480.

## Что сделать (направление, не предписание)

Нативы `_lumen_f_document_open/write/close(bid, …)`, выполняющие те же шаги в рантайме фрейма (его
`document.open/write/close` — тем же кодом, что у главного документа), и методы фасада поверх них;
`readyState` фасада — из состояния фрейма, а не константой.

## Как проверить

- Юнит-тест в `crates/js` (рядом с тестами `frame_bridge`): родитель делает
  `f.contentWindow.document.open(); write('<p id=x>hi</p>'); close()` — затем
  `f.contentWindow.document.getElementById('x').textContent === 'hi'`.
- `tests/wpt/run_corpus.py --prefixes quirks` — нет ERROR `document.open is not a function`;
  балл категории выше 17.83.
