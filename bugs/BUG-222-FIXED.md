# BUG-222 — WASM instance registry not cleared on JS context teardown

**Статус:** FIXED 2026-06-19
**Компонент:** js / shell
**Обнаружен:** 2026-06-18 (при работе над U-4 i64/BigInt-маршалингом)
**Исправлен:** 2026-06-19 (B-1, ADR-014 — JS-рантайм на выделенном потоке)

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

## Решение (B-1 / ADR-014)

С переносом QuickJS-рантайма на выделенный поток (`QuickJsRuntime` стал
хэндлом, `Runtime`/`Context` живут и дропаются в `js_thread_main`,
`crates/js/src/lib.rs`) появился явный teardown на том самом потоке, которому
принадлежит thread-local `REGISTRY`. `js_thread_main` после выхода из
command-loop (по `JsCommand::Shutdown` из `Drop` или закрытию канала) вызывает
`wasm::clear_registry()` **до** дропа `Inner` — `Persistent`-импорты
освобождаются, пока `Runtime` ещё жив, и GC-проверка `list_empty(&rt->gc_obj_list)`
проходит. Прежний handoff в шелл больше не нужен: реестр thread-local к JS-потоку,
а не к UI-потоку, поэтому очистка обязана идти именно там.

## Обходной путь (исторический)

До фикса юнит-тесты `with_wasm` звали `clear_registry()` после `f(&ctx)`, пока
контекст ещё жив. Продакшен-путь закрыт переносом teardown в `js_thread_main`.
