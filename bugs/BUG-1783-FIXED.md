# BUG-1783 — ea.com: окно умирает — `bad_optional_access was thrown in -fno-exceptions mode`

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (`crates/js/src/v8_runtime/script_attribution.rs` — чтение строк V8); мина — upstream V8 (`src/objects/string.cc`, `String::SlowGetFlatContent`)

## Источник: сайты

ea-com (вес 2, статус DEAD оба раза, `text_len` 0 при 7986 у Chrome) — **1 сайт, вес кластера 2**; воспроизведено 2 из 2 запусков (базовый прогон и повтор с `--restart-every 1`). Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

## Симптом

Живой журнал (`live.stderr.15.log`): картинки drop-assets.ea.com грузятся, затем `[JS error] Uncaught TypeError: Cannot read properties of undefined (reading 'call')` (4 раза, webpack-рантайм), `window.parent.handleToken is not a function` (auth-iframe `static-pages/auth.html#error=login_required`), `Battery Status API is not supported` и последней строкой — `bad_optional_access was thrown in -fno-exceptions mode`: V8 делает abort, окно мертво (`lumen_status` DEAD, `ready` не наступает).

## Локализация (2026-11-14, P3)

Симптом сводится к headless-репро: `--dump-layout https://www.ea.com/` падает тем же fastfail
(`exit=0xC0000409`), 3/3 прогонов. Стека в релизе нет — снят самописным мини-отладчиком
(DEBUG_ONLY_THIS_PROCESS + dbghelp `StackWalk64` + stack-scan; инструмент в `D:\Temp\opencode\v8crash.py`,
в репо не влит). Кадры падения (младшие сверху):

```
std::__Cr::__throw_bad_optional_access
v8::internal::String::SlowGetFlatContent+0x179   ← отсюда сделан бросок
v8::String::ValueView::ValueView+0x14a           ← ctor читает строку
…IC-клей (StoreIC / DefineNamedOwnIC_Miss / Builtins_CallApiCallbackGeneric)
to_rust_string_lossy (rusty_v8)
capture_call_site+0x138                          ← наш колбэк `_lumen_capture_call_site`
```

Механизм — upstream V8, жив и в main: констрейнт cons-строки, которую сам V8 считает flat
(`ConsString::IsFlat()` = «`second` пуст»), но чей `first` остаётся indirect (cons/sliced).
`ValueView`-ctor идёт по быстрой ветке (`IsFlat()`), `GetFlatContent` → `TryGetFlatContentFromDirectString`
даёт `nullopt` → `SlowGetFlatContent` снимает один уровень, но после него строка остаётся indirect, и
финальный **незащищённый `.value()`** бросает `std::bad_optional_access`; при сборке с `-fno-exceptions`
это terminate-handler → abort → fastfail. (`String::Flatten` на той же форме возвращает inner-cons
и не чинит — канонизатор `NewConsString` пустой `right` схлопывает, а внутренние пути V8 такую
форму создают; сам `ValueView` в 15.0 неспособен её прочитать.)

Триггер на ea.com: `_lumen_capture_call_site` (LONGTASK-1 срез 4/5) читает `get_script_origin().resource_name()`
и `get_name()` каждого записываемого колбэка — имена функций Next.js/webpack приходят cons-формами.

## Фикс

`script_attribution.rs` обе строки читает через новый хелпер `string_lossy_via_write` —
`String::WriteV2` → `i::String::WriteToFlat` (обход cons-дерева стеком, корректен для любой формы,
в flat-content-механику не заходит вовсе). Цена — одна конверсия UTF-16→UTF-8 на чтение.

`to_rust_string_lossy` в остальных биндингах **не** переписан (пара тысяч вызовов): на
конструируемых из API формах он безопасен, смертельная форма нашей поверхностью не
строится и до сих пор замечена только на чтении имён функций. Если всплывёт второй
сайт с тем же abort — переводить чтение на `string_lossy_via_write` по мере подтверждения
(см. «Как проверить»).

## Как проверить

- Юнит-тест: `cargo test -p lumen-js --features v8-backend --lib string_read_via_write_matches_content_for_every_shape`
  — хелпер даёт корректное содержимое на seq/cons/sliced/thin формах (сам процесс жив — тоже часть ассерта).
- Живой: `LUMEN_NO_ADBLOCK=1 lumen --dump-layout https://www.ea.com/` — до фикса fastfail 3/3,
  после — 3/3 чистых выхода (exit=0), `bad_optional` в stderr нет.
