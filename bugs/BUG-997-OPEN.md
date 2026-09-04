# BUG-997 — `native_binding_panic_does_not_abort_process` больше не проверяет то, что заявлено в имени

**Статус:** OPEN
**Заведён:** 2026-09-04 (P1, DATAURL-1 — обнаружено гейтом `scoped-test.sh`, к data: URL отношения не имеет)
**Область:** `crates/js/src/dom/tests/v8_perf_typedom_node.rs:726` (`native_binding_panic_does_not_abort_process`)
**Владелец:** P3/P4 (тест-фоллоу-ап на правку BUG-986)

## Симптом

Тест красный детерминированно (не флак — воспроизводится в полной изоляции,
`cargo test -p lumen-js --lib --features v8-backend -- native_binding_panic_does_not_abort_process --exact`,
как на `p1-dataurl1-data-scheme`, так и на чистом `main` HEAD `701f9bda5`):

```
assertion `left == right` failed
  left: String("")
 right: String("Error")
```

## Root cause

Тест — регресс-тест BUG-418 (панике внутри native-биндинга не давать ронять процесс
через `extern "C"` границу V8, `catch_unwind` в `native_fn_trampoline` ловит и
превращает в JS `Error`). Он провоцировал панику намеренно: звал
`_lumen_append_child(0, 4294967295)` с заведомо чужим `NodeId`, что раньше падало
внутри `Document::get`/`get_mut` (`&self.nodes[id.index()]`, `index out of bounds`).

Коммит `869130d4f` (BUG-986, 2026-09-04) изменил именно этот путь: `_lumen_append_child`
и соседние нативы дерева (`dom_core.rs`) теперь сперва проверяют оба id через
`Document::contains_id` и на чужом id **не паникуют**, а тихо пропускают операцию с
диагностикой в stderr (`[BUG-986] _lumen_append_child parent: NodeId … вне арены
документа (len …) — операция пропущена`) — что видно и в выводе этого теста. Вызов
`_lumen_append_child(0, 4294967295)` больше не паникует вообще, `catch_unwind`
никогда не срабатывает, `caught` остаётся `''`.

Тест не устарел концептуально (гарантия BUG-418 — что панике в native-биндинге не
дают уронить процесс — всё ещё нужна как инвариант), но конкретный триггер, который
он использовал, BUG-986 намеренно закрыл превентивной проверкой раньше границы
паники. Нужен другой путь, реально доходящий до `catch_unwind` (native-биндинг без
BUG-986-guard'а, либо синтетическая паника через тестовый хук), либо тест
переписывается на проверку самого guard'а (`caught` содержит текст про
`вне арены`/аналог), а сохранение инварианта BUG-418 проверяется отдельно.

## Влияние

Только на `-p lumen-js --lib --features v8-backend` (полный набор фич, куда
`scoped-test.sh` почти всегда затягивает `lumen-js`, ср. известный класс
[[project_lumenjs_worker_tests_red_on_main]]) — под дефолтным набором фич теста нет
вообще (`v8-backend` off by default), поэтому `cargo test -p lumen-js --lib` без фичи
его не видит и гейт кажется зелёным локально, если не гонять полный `scoped-test.sh`
или явный `--features v8-backend`. Красит `/lumen-task-finish` для любой ветки,
независимо от того, что она правит.

## Сырые данные

Прогон `scoped-test.sh main` на `p1-dataurl1-data-scheme` (правка DATAURL-1,
`crates/network/src/lib.rs` — `lumen-js` не тронут вообще) и повторно на чистом
`main` HEAD `701f9bda5` — идентичный результат в обоих случаях, значит поломка не
связана с DATAURL-1 и присутствует на `main` уже сейчас.
