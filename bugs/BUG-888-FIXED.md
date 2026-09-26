# BUG-888 — `document.open()` и `document.close()` отсутствуют целиком (`document.write` при этом есть)

**Статус:** FIXED 2026-09-21 (P6, GAP-DOCWRITE)
**Тип:** нереализованная функциональность, не дефект реализованного кода — велась как задача `GAP-DOCWRITE` в [ROADMAP.md](../ROADMAP.md). Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, вариант `doc-write`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — блок `document.write`/`writeln`/`open`/`close`)
**Владелец:** P6.

## Симптом

```
document.open   → undefined
document.close  → undefined
document.write  → function
document.writeln→ function
```

Вызов `document.open()` бросает `TypeError: document.open is not a function`
первой же строкой, поэтому весь набор `dynamic-markup-insertion/opening-the-
input-stream/*` не начинается вовсе.

Поведение `document.write` после `load` — осознанный no-op
([BUG-701](BUG-701-FIXED.md): вместо спекового разрушительного неявного
`document.open()` текст просто отбрасывается), и это подтверждено замером:
`document.write("<p id=w1>")` после `load` не вставляет узел
(`found=false`) и не исполняет записанный `<script>`. Но именно поэтому
`document.open()` нужен как отдельная точка входа: тест, который открывает
поток явно, сейчас не может ни начать, ни проверить замену документа.

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant doc-write --variant
doc-open` (2026-08-23, dev-release, Linux, `main` = `0dc60692d`):

```
doc-write  ticks=17  dmi open=undefined close=undefined write=function writeln=function
                     wrote-plain found=false
                     wrote-script-tag
                     doc-write-alive ready=complete
doc-open   ticks=15  docopen-threw TypeError: document.open is not a function
                     docopen-alive found=false
```

`wrote-script-ran` не напечатан — это половина [BUG-568](BUG-568-FIXED.md),
которая после BUG-701 стала следствием сознательного no-op, а не отдельным
дефектом исполнения.

## Цена по WPT

Один id остатка WPT-RUN-5:
`html/webappapis/dynamic-markup-insertion/opening-the-input-stream/document.open-03.html`
(«document.open and no singleton replacement»). Вся папка
`opening-the-input-stream/` (~30 id) не вендорена — цена по остатку нижняя.

## Исправлено

`document.open()`/`.close()` добавлены в `web_api_shim_mid.js` как явная
точка входа (HTML LS §8.4.4), не полная спековая модель «erase a document»
(свежий `Document`, новый парсер — план из §Что дальше выше):

- `open()` — no-op, если ещё идёт исходный парсинг (`readyState==='loading'`,
  то же условие, что и у ранее принятого no-op в `write()`); иначе снимает
  всех детей `<body>` и возвращает `readyState` в `'loading'`, снова открывая
  окно, в которое `write()` может вставлять;
- `close()` — no-op без предшествующего `open()` (нет точки вставки, ставить
  которую некому); иначе переигрывает `_lumen_apply_ready_state('interactive')`
  → `('complete')`, что заново рассылает `readystatechange`/
  `DOMContentLoaded`/`load` — те же обработчики, что и у обычной загрузки.

Сознательно вне рамок, как и no-op `write()` после `load` из BUG-701:
`document.open-03` (замена документа как singleton-объекта) и вся папка
`opening-the-input-stream/` (~30 id) остаются не вендорены — это отдельное
решение о полной модели динамической вставки разметки, не о самом наличии
`open()`/`close()`.

5 новых юнит-тестов (`document_open_reenables_write_after_complete`,
`document_open_clears_body`, `document_close_fires_dcl_and_load_again`,
`document_close_without_open_is_noop`, `document_open_while_loading_is_noop`)
в `crates/js/src/dom/tests/v8_page_visibility_beacon.rs`. `cargo test -p
lumen-js --features v8-backend` 4072/4072, `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` чист.
