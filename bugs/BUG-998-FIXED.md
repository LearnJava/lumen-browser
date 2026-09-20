# BUG-998 — `history.back()`/`forward()` через JS не доводят document_ready, следующий `eval` теряет JS-контекст

**Статус:** FIXED 2026-09-20 (P3)
**Заведён:** 2026-09-04 (смоук-тест нового режима `--mode session` в `scripts/perf_audit.py`, AUDIT-1)
**Область:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — `_lumen_apply_ready_state`/новая `_lumen_mark_ready_state_restored`), shell (`crates/shell/src/lumen/bfcache.rs::bfcache_thaw`)

## Симптом (как заведён)

Кросс-документный `history.back()`/`history.forward()`, вызванный со стороны
страницы (`eval` через MCP, не шелл-навигация), после навигации между двумя
разными сайтами:

1. `history.back()` выполняется без ошибки;
2. последующий `wait document_ready` (таймаут 5с) НЕ дожидается готовности —
   `Wait error: wait timeout: DocumentReady`;
3. следующий `eval` на той же вкладке падает с `Eval error: JS context not
   available`.

Воспроизведено детерминированно:

```
python scripts/perf_audit.py --mode session --only example --only hn \
    --session-tabs 2 --dwell 1 --scroll-ticks 1 --timeout 40
```

## Корень

Живой пробой (`eprintln!`-инструментация в `check_wait_condition`,
`navigate_back`, `apply_loaded_page`, `bfcache_thaw`, впоследствии снятая) —
подтверждено пошагово:

1. `history.back()` резолвится в `navigate_back`, который находит для
   `example.com` **Frozen bfcache-запись** (сохранённую при исходной
   навигации `example.com` → `hn`) и берёт путь `bfcache_thaw` — НЕ
   `reload()`. Это уже само по себе означает, что фикс BUG-835 (`ParkedPage`)
   тут ни при чём: тот путь для этой страницы не участвует (`example.com`
   никогда не была запаркована — `park_current_page` не вызывался при первой
   навигации, чтобы её сохранить как parked).
2. `bfcache_thaw` (`crates/shell/src/lumen/bfcache.rs`) при оттайке
   Frozen-записи ставит **совершенно новый** V8-рантайм (`V8JsRuntime::new()`
   + `install_dom(...)`) поверх десериализованного DOM. Свежий рантайм
   заново исполняет `WEB_API_SHIM`, в котором `_doc_ready_state` — обычная
   `var`, инициализированная `'loading'` (`web_api_shim_mid.js:10354`).
3. Единственный механизм, продвигающий `_doc_ready_state` вперёд —
   `_lumen_apply_ready_state('interactive'|'complete')`
   (`web_api_shim_tail_b.js`), вызываемый ТОЛЬКО из обычного pipeline'а
   загрузки страницы: `notify_dom_content_loaded`/`notify_window_loaded`
   (`persistent_js.rs:1140-1145`), с call-site'ами в `page_pipeline.rs`
   (парсинг) и `page_load.rs::apply_loaded_page` (все ресурсы загружены).
   **`bfcache_thaw` не вызывает ни одну из этих функций ни разу** — оттайка
   не идёт через `apply_loaded_page`/`render_bytes`, это отдельный, короткий
   путь. Инструментация (временный `console.log` внутри
   `_lumen_apply_ready_state`) подтвердила: для оттаянного `example.com` эта
   функция не вызывается вовсе.
4. Результат: `document.readyState` навсегда застревает на `'loading'` для
   любой Frozen-оттаянной страницы. `wait document_ready` (гейтится на
   `readyState === "complete"`, `crates/shell/src/lumen/automation.rs`)
   поэтому ждёт весь таймаут и падает — не потому что страница «не успела»,
   а потому что она никогда не успеет: событие, которое довело бы её до
   `complete`, не наступит никогда.
5. `history.forward()`, вызванный сразу вслед (по логике смоук-теста —
   таймаут не считается фатальным, следующий шаг всё равно идёт), находит
   для `hn` уже не Frozen-запись (та осталась только за исходной навигацией
   `example.com`→`hn`; сам обратный обход её не создаёт), а действительно
   уходит в полный сетевой `reload()` — у него собственная, отдельная (и
   корректная) асинхронность, здесь не дефект.

**Потеря JS-контекста, заявленная в исходном тексте бага** («следующий eval
падает `JS context not available`») в точном воспроизведённом сценарии
оказалась **отдельным, не связанным дефектом**: шаг `_session_frame` в
`iter_session()` (`scripts/perf_audit.py`) падает из-за того, что
`window.open('about:blank')` в шаге `_session_popup` — в этом
MCP-однооконном автоматизаторе (нет `switch_tab`/`close_tab`, задокументировано
в самом скрипте) — **навигирует АКТИВНУЮ вкладку** на `about:blank` вместо
открытия отдельного окна, а `about:blank` не несёт JS. Тот же провал
(`_session_frame`/`_session_close_last_tab`) воспроизводится независимо от
фикса ниже — это существующее, задокументированное ограничение тулинга, не
регрессия BUG-835 и не часть этого бага.

## Фикс

`crates/js/src/shim/web_api_shim_tail_b.js`: новая функция
`_lumen_mark_ready_state_restored()` — ставит `_doc_ready_state = 'complete'`
**напрямую, без диспатча событий**. Вызывать существующий
`_lumen_apply_ready_state('complete')` было бы неверно по спеке: HTML LS §8.6
«reactivate a document» требует, чтобы восстановленная страница получила
только `pageshow(persisted=true)`, а `readystatechange`/`DOMContentLoaded`/
`load` не переигрывались — они уже случились один раз при исходной загрузке.
`_lumen_apply_ready_state('complete')`, в отличие от новой функции,
диспатчит `readystatechange` и слушателей `load`, что стало бы отдельным
дефектом (лишний `load` при возврате из bfcache).

`crates/shell/src/lumen/bfcache.rs::bfcache_thaw`: после успешного
`install_dom` зовёт `rt.eval("_lumen_mark_ready_state_restored()")`, до
`self.set_js_ctx(...)`.

`restore_parked_page` (путь BUG-835, живой рантайм) правки не требует —
рантайм там свой, не пересоздаётся, `_doc_ready_state` уже верно стоит на
`'complete'` с исходной загрузки.

## Проверка

Тот же репро-сценарий (`perf_audit.py --mode session`, живое окно,
`dev-release`) после фикса: `_session_back_forward` — `OK` (было `DEGRADED`,
`Wait error: wait timeout: DocumentReady`), трижды подряд. `_session_frame`/
`_session_close_last_tab` остаются `DEGRADED` по независимой причине
(`window.open` описана выше) — без изменений до и после фикса, отдельная
задача при желании её закрыть (не заведена отдельным багом: это осознанный
предел тулинга AUDIT-1, зафиксированный в самом `perf_audit.py`).

`cargo clippy -p lumen-shell -p lumen-js --all-targets -- -D warnings` —
чисто.
