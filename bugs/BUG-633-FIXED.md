# BUG-633 — V8 fatal crash `Check failed: isolate_->IsOnCentralStack()` in `lumen_js::v8_runtime::from_v8`

**Статус:** FIXED 2026-08-05
**Компонент:** js (`crates/js/src/v8_runtime.rs::from_v8`)
**Найден:** 2026-08-05, P2, WPT-VENDOR-media-source

## Симптом

Живой процесс `lumen.exe` падает целиком (V8 `V8_Fatal`, не Rust `panic!`) во
время выполнения ряда `tests/wpt/media-source/mediasource-*.html`. Со стороны
`wptrunner`/WebDriver BiDi это выглядит как
`webdriver.bidi.error.UnknownErrorException: unknown error (WebSocket connection
closed)` — соединение с браузером рвётся, потому что процесс браузера умер.

Воспроизводится детерминированно (не артефакт параллельного запуска — тот же
набор из 13 файлов падает и на `--processes=1`, и на `--processes=4`):

```
mediasource-addsourcebuffer-mode.html
mediasource-addsourcebuffer.html
mediasource-changetype.html
mediasource-closed.html
mediasource-correct-frames-after-reappend.html
mediasource-correct-frames.html
mediasource-endofstream-invaliderror.html
mediasource-h264-play-starved.html
mediasource-invalid-codec.html
mediasource-multiple-attach.html
mediasource-removesourcebuffer.html
mediasource-sourcebufferlist.html
mediasource-timestamp-offset.html
```

Итог прогона категории (`run_report.py --all --root media-source --recursive`,
и с `--processes=1`, и с `--processes=4`, идентично): 60/82 harness OK,
0/272 подтестов пройдено (MediaSource API не реализован вовсе — отдельный,
уже задокументированный пробел, не предмет этого бага).

## Крашдамп (stderr браузера)

```
#
# Fatal error
# Check failed: isolate_->IsOnCentralStack().
#
#FailureMessage Object: 000000CE55B4B6B0
==== C stack trace ===============================

v8::base::debug::StackTrace::StackTrace [...]
v8::platform::DefaultPlatform::GetStackTracePrinter [...]
V8_Fatal [...]
v8::internal::Heap::CollectGarbage [...]
v8::internal::HeapAllocator::CollectGarbageAndRetryAllocation [...]
v8::internal::HeapAllocator::RetryCustomAllocate [...]
v8::internal::HeapAllocator::AllocateRawSlowPath [...]
v8::internal::Factory::AllocateRaw [...]
v8::internal::FactoryBase<v8::internal::Factory>::NewFixedArrayWithFiller [...]
v8::internal::FactoryBase<v8::internal::Factory>::NewFixedArrayWithMap [...]
v8::internal::OrderedHashTable<v8::internal::OrderedHashSet,1>::Allocate [...]
v8::internal::KeyAccumulator::AddKey [...]
v8::internal::KeyAccumulator::CollectOwnPropertyNames [...]
v8::internal::KeyAccumulator::CollectOwnKeys [...]
v8::internal::KeyAccumulator::CollectKeys [...]
v8::Object::GetPropertyNames [...]
v8::Object::GetOwnPropertyNames [...]
v8__Object__GetOwnPropertyNames [...]
lumen_js::v8_runtime::from_v8 [+673]
lumen_js::v8_runtime::from_v8 [+1047]   <- repeats, dozens of frames
lumen_js::v8_runtime::from_v8 [+1047]
...
```

`isolate_->IsOnCentralStack()` — внутренний V8-чек: GC (или иная операция,
трогающая кучу) была вызвана в контексте, где V8 не считает себя «на
центральном стеке» (обвязка вокруг stack-switching для `Isolate`, см.
`v8/src/execution/isolate.h`). Это фатальная проверка V8, не наш assert —
`V8_Fatal` завершает процесс немедленно, `TryCatch` его не ловит.

## Наблюдения (не root cause — для P3)

- Крах происходит очень рано в загрузке следующей страницы: сразу после
  `Reload:`/стриминга `testharness.js`+`testharnessreport.js`+
  `mediasource-util.js`, до того как успевает выполниться собственный код
  теста (видно по логам живого окна — между «загружены скрипты» и первым
  крашдампом проходит ~0.1с).
- Не привязан к предыдущему тесту: одни и те же 13 файлов падают независимо
  от того, чем закончился предыдущий тест в очереди (`Test OK`, `TIMEOUT`
  или сам краш) — то есть детерминирован содержимым/порядком загрузки именно
  этих 13 страниц, не состоянием, унаследованным от соседа.
- `from_v8` (`crates/js/src/v8_runtime.rs:4981`) рекурсивно обходит массивы
  и объекты (`val.is_array()`/`val.is_object()` ветки) без ограничения
  глубины и без защиты от циклов — стек падает на `GetOwnPropertyNames`
  внутри этой рекурсии, так что похоже на конвертацию достаточно глубокого
  /большого JS-объекта (вероятно, из обработчика необработанного исключения
  или отклонённого промиса — остальные `FAIL`-тесты в этой же категории
  показывают `Unhandled rejection with value: object "ReferenceError:
  MediaSource is not defined"`, конвертация значения отклонения в `JsValue`
  — вероятный путь вызова `from_v8`, но не подтверждён трассировкой стека
  выше `from_v8`, т.к. C-стек-принтер не размотал Rust-фреймы вызывающей
  стороны).
- Требует объяснения именно «на каком стеке» вызывается `from_v8` в этом
  месте — GC-триггер сам по себе безобиден миллионы раз в секунду в норме;
  фатально именно то, что происходит НЕ на central stack.

## Как повторить

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py \
    --all --root media-source --recursive --processes=1 \
    --out .tmp/wpt-media-source.html
# смотреть live-браузер stderr в интерливед-логе (не в HTML-отчёте) —
# `#FailureMessage`/`isolate_->IsOnCentralStack()` виден только там.
```

## Не путать с

Отсутствием MediaSource API как такового (`ReferenceError: MediaSource is not
defined` на остальных 69 harness-OK тестах категории) — это отдельный,
задокументированный Phase-0 пробел (`tests/wpt/VENDOR.md` запись
`media-source`), не предмет этого бага. Этот баг — про сам процесс браузера,
падающий насмерть, а не про недостающий API.

## Фикс (P3, 2026-08-05)

`from_v8` рекурсивно обходил JS-массивы/объекты (`is_array()`/`is_object()`
ветки) без единой защиты от глубины или циклов — любой JS-объект с
самоссылкой (например, пара `test`/`EventExpectationsManager` из
`testharness.js`, где `test.eventExpectations_.test_ === test`, ровно та
форма, что создаёт `addExtraTestMethods()` для каждого `mediasource_test`)
уводил обход в бесконечную рекурсию по C-стеку; ровно на этой глубине
очередной вызов `GetOwnPropertyNames` триггерил GC, который V8 фатально
проверяет на «центральный стек» — отсюда `V8_Fatal`, не перехватываемый
`TryCatch`. Точный вызывающий путь (какое именно JS-значение конвертируется
в этот момент — скорее всего значение необработанного promise-rejection или
завершающее выражение самого `<script>`, чья completion value — возвращённый
`Test`-объект) не был подтверждён трассировкой стека выше `from_v8`, но
устранение самой возможности бесконечной/сколь угодно глубокой рекурсии над
JS-контролируемым графом объектов — правильный фикс независимо от того, какой
конкретно путь его вызывает: любой обход JS-графа, где узлы выбирает
атакующая/недоверенная сторона (произвольная JS-страница), обязан быть
устойчив к циклам и глубине сам по себе.

**Реализация:** `from_v8` теперь тонкая обёртка над `from_v8_bounded`,
которая ведёт `ancestors: Vec<NonZeroI32>` — стек identity-хэшей
(`Object::get_identity_hash()`) объектов/массивов на текущем пути обхода
(push при входе в ветку `is_array()`/`is_object()`, pop на выходе). Если
хэш нового объекта уже есть в `ancestors` — это цикл, возвращается
`JsValue::String("[Circular]")` вместо рекурсии. Отдельно ограничена и
абсолютная глубина (`FROM_V8_MAX_DEPTH = 64`) — длинная ациклическая цепочка
таким же образом выходит по `JsValue::String("[Max Depth Exceeded]")` вместо
продолжения рекурсии до исчерпания стека. Оба маркера — обычные JS-совместимые
строки; новый вариант `JsValue` не понадобился.

Регресс-тесты `eval_circular_object_does_not_crash` (self-referencing
объект) и `eval_deeply_nested_array_truncates` (200 вложенных массивов,
кап 64) в `v8_runtime.rs::tests`. `cargo test -p lumen-js --features
v8-backend` — 75/75 зелёных (было 73 до добавления двух новых).

**Живая перепроверка** (репро-рецепт из раздела «Как повторить» выше, на
свежей `dev-release` сборке с фиксом): `run_report.py --all --root
media-source --recursive --processes=1` — **73/82 harness OK** (было 60/82),
0/337 подтестов (ожидаемо — MediaSource API по-прежнему не реализован, см.
«Не путать с»). Все 13 ранее падавших файлов (`mediasource-addsourcebuffer-mode`,
`mediasource-addsourcebuffer`, `mediasource-changetype`, `mediasource-closed`,
`mediasource-correct-frames-after-reappend`, `mediasource-correct-frames`,
`mediasource-endofstream-invaliderror`, `mediasource-h264-play-starved`,
`mediasource-invalid-codec`, `mediasource-multiple-attach`,
`mediasource-removesourcebuffer`, `mediasource-sourcebufferlist`,
`mediasource-timestamp-offset`) отработали до конца под одним и тем же
`pid` без единого `V8_Fatal`/`FailureMessage` в логе — прирост harness OK
ровно +13, точное совпадение со списком. Один процесс-рестарт (`pid:1940`→
`pid:4256`) действительно произошёл в этом прогоне, но на границе с
`mediasource-encrypted-webcodecs-appendencodedchunks-play.https.html`
(TLS `UnknownIssuer`, уже задокументированный отдельный гэп across многих
категорий, не относится к этому багу) — не на одном из 13 файлов этого бага.
