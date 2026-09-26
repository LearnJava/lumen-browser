# BUG-1196 — SIGSEGV на первом `eval`/`new Function` под Linux: хук codegen в MSVC-форме

**Статус:** FIXED 2026-09-27 (P4)
**Заведён:** 2026-09-27 (P4, по ходу `background-attachment`: CPU-эталоны не генерировались).
**Область:** js (`crates/js/src/v8_runtime/codegen_hook.rs`, `crates/js/cpp/codegen_callback.cc`).

## Симптом

На Linux (x86-64) любой процесс с V8, где страница или шим выполняет `eval`/`new Function`,
падает с `SIGSEGV`. Под gdb падение в `codegen_hook.rs:136`, внутри
`modify_code_generation_from_strings`. На чистом `main` (f345a2fbe) падают:

- `cargo test -p lumen-driver --features cpu-render --test all cases::snapshot_cpu`:
  CPU-эталоны нельзя ни проверить, ни пересобрать;
- `cargo test -p lumen-js --features v8-backend --lib tt_codegen`;
- `cargo test -p lumen-shell`: тест-бинарь `lumen` падает посреди прогона.

На Windows (MSVC) ошибки нет.

## Причина

TRUSTEDTYPES-1 срез 7 (c71846072) регистрирует callback
`SetModifyCodeGenerationFromStringsCallback`, у которого C++-тип возвращает
`ModifyCodeGenerationFromStringsResult {bool, MaybeLocal<String>}` по значению. Rust-сторона
реализовала его только в форме MSVC x64: скрытый указатель на результат идёт первым
аргументом, остальные сдвинуты на один. На Windows это подтвердили dumpbin-дизассемблером.

В Itanium C++ ABI (System V x86-64, AArch64) структура с default member initializer остаётся
trivially copyable *for the purpose of calls*. Нетривиален только конструктор по умолчанию,
а копирование, перемещение и деструктор тривиальны. Поэтому 16-байтная `{bool, ptr}`
возвращается в RAX:RDX, а V8 передаёт три настоящих аргумента в RDI/RSI/RDX. Callback принимал
`context` за `out`, `source` за `context` и так далее, строил scope из мусора и писал результат
по чужому адресу.

## Исправление

Сигнатура callback теперь выбирается по целевой платформе (`RawCallback`):

- для `windows` + `msvc` остаётся прежняя форма со скрытым указателем;
- для остальных платформ Rust-функция `extern "C"` возвращает `#[repr(C)] RawResult` по значению,
  а это и есть форма System V / AAPCS64.

Тело вынесено в общую `codegen_result`. C++-трамплин не менялся: он сохраняет указатель
как есть.

## Проверка

- `cargo test -p lumen-js --features v8-backend --lib`: 4528/4528, из них 8 `tt_codegen_*`.
- `cargo test -p lumen-driver --features cpu-render --test all`: 204/204, `snapshot_cpu`
  проходит, эталоны пересобираются.
- `cargo test -p lumen-shell` доходит до конца. Остались 6 падений, их причины: Windows-пути
  (`D:/…`, `\\`) и CSP для `file:`-листов на Linux. Эти 6 на `main` не видны только потому, что
  бинарь падал раньше. К этому багу они не относятся.
