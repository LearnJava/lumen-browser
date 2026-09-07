# BUG-1030: `native_binding_panic_does_not_abort_process` устарел после guard-ов BUG-986 — `cargo test -p lumen-js` красный на main

**Статус:** FIXED 2026-09-08 (P3)
**Дата:** 2026-09-07
**Компонент:** js (тест `crates/js/src/dom/tests/v8_perf_typedom_node.rs:848`
`native_binding_panic_does_not_abort_process`)
**Найден:** 2026-09-07, при прогоне гейта по задаче [BUG-1027](BUG-1027-FIXED.md)

## Симптом

```
---- dom::tests::v8_perf_typedom_node::native_binding_panic_does_not_abort_process stdout ----
[BUG-986] _lumen_append_child parent: NodeId 0 вне арены документа (len 9) — операция пропущена
[BUG-986] _lumen_append_child child: NodeId 4294967295 вне арены документа (len 9) — операция пропущена
assertion `left == right` failed
  left: String("")
 right: String("Error")
```

`cargo test -p lumen-js --lib --features v8-backend` → 3535 passed, **1 failed**.
Воспроизводится на чистом `main` (проверено на 4f914edad в главном worktree),
то есть гейт красный у всех, кто его гоняет, а не следствие чьей-то ветки.

## Механизм

Тест — регрессия на [BUG-418](BUG-418-FIXED.md): невалидный `NodeId`, дошедший
до `Document::get`, паниковал внутри `extern "C"`-границы V8, Rust отказывался
разматывать стек через неё и валил процесс. Фикс обернул нативный диспетч в
`catch_unwind`, и тест проверяет, что паника доезжает до JS как ловимая ошибка:
`_lumen_append_child(0, 4294967295)` → `catch (e) { caught = e.name }` →
ожидается `"Error"`.

С тех пор [BUG-986](BUG-986-FIXED.md) добавил проверку границ арены **до**
обращения к документу: невалидный `NodeId` больше не паникует, а логируется и
операция пропускается. Исключения нет, `caught` остаётся `""` — тест падает,
хотя обе защиты на месте и обе работают.

## Что надо сделать

Решить, что тест должен проверять теперь, и переписать под это:

- проверка `catch_unwind` на границе V8 (предмет BUG-418) нужна по-прежнему, но
  ей нужен вход, который **всё ещё** паникует — guard BUG-986 закрыл именно тот,
  что использовался;
- поведение «невалидный `NodeId` тихо пропускается» (предмет BUG-986) стоит
  закрепить отдельным утверждением, иначе оно не покрыто ничем.

Просто поменять ожидание на `""` — потерять регрессию BUG-418 целиком: тест
станет тавтологией, проходящей и без `catch_unwind`.

## Исправление 2026-09-08 (P3)

`native_binding_panic_does_not_abort_process` переведён на другой вход:
`_lumen_get_tag_name(4294967295)` вместо `_lumen_append_child(0,
4294967295)`. `_lumen_get_tag_name` (`crates/js/src/v8_runtime/install/dom_core.rs`)
всё ещё зовёт `doc.get(nid)` напрямую, без `contains_id`/`try_get` — в
отличие от `_lumen_append_child`, которую BUG-986 обвязал bounds-check'ом,
эта функция не получила такой защиты и по-прежнему паникует
(`Document::get`/`foreign_id_panic`) внутри `extern "C"`-границы V8, которую
ловит `catch_unwind` в `native_fn_trampoline` — регресс BUG-418 остался
покрыт живым входом, а не тавтологией.

Поведение BUG-986 (тихий пропуск чужого `NodeId`) закреплено отдельным
новым тестом `native_binding_foreign_node_id_is_silently_skipped` — тот же
вызов `_lumen_append_child(0, 4294967295)`, что раньше использовался в этом
тесте, теперь проверяет, что `caught` остаётся `""`.

**Тесты:** `cargo test -p lumen-js --lib --features v8-backend`: 3541/3541.
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` и `cargo clippy --workspace --all-targets -- -D warnings`: чисто.
