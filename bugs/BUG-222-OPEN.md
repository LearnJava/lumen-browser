# BUG-222 — WASM instance registry not cleared on JS context teardown

**Статус:** OPEN
**Компонент:** js / shell
**Обнаружен:** 2026-06-18 (при работе над U-4 i64/BigInt-маршалингом)

## Симптом

`lumen-js::wasm` хранит скомпилированные модули и живые инстансы в
thread-local `REGISTRY` (`crates/js/src/wasm/mod.rs`). Инстанс с функцией-импортом
держит `Persistent<Function>` — ссылку на JS-функцию в контексте QuickJS.

`REGISTRY` — `static`, он не очищается при разрушении `rquickjs::Runtime`.
Если страница инстанцировала WASM-модуль с импортами, а затем контекст
уничтожается (навигация/закрытие вкладки), «утёкший» `Persistent` держит GC-объект
живым → QuickJS падает на assertion `list_empty(&rt->gc_obj_list)`
(`quickjs.c`) с `STATUS_STACK_BUFFER_OVERRUN` при teardown.

Воспроизводится в тесте: первый JS-интеграционный тест, регистрирующий
импорт-`Persistent` (`webassembly_i64_import_arg_and_result_use_bigint`), ронял
весь тестовый бинарь на выходе, пока не добавили `wasm::clear_registry()` в
`with_wasm`-харнес.

## Причина

Нет хука «контекст разрушается → очистить WASM-реестр этого потока».
`pub fn clear_registry()` уже добавлен в `crates/js/src/wasm/mod.rs` и
освобождает все `Persistent` (модули + инстансы), но в шелле он нигде не вызывается.

## Что сделать (handoff P3 — shell)

Вызвать `lumen_js::wasm::clear_registry()` при разрушении/замене JS-контекста
страницы (там же, где дропается `PersistentJs` / `rquickjs::Runtime`, см.
`crates/shell/src/main.rs` `QuickPersistentJs`). После этого реестр текущего
потока пуст до того, как QuickJS пройдёт teardown-проверку GC.

## Обходной путь (сейчас)

В юнит-тестах `with_wasm` зовёт `clear_registry()` после `f(&ctx)`, пока контекст
ещё жив. Продакшен-путь остаётся незакрытым до wiring в шелл.
