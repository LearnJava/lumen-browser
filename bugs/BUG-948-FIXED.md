# BUG-948 — `longtask`/`long-animation-frame` отсутствуют из `supportedEntryTypes`: `observe()` тихо не принимает ничего

**Статус:** FIXED 2026-09-25 (P1, LONGTASK-1 срезы 1-5)
**Тип:** нереализованная функциональность — обе группы `PerformanceEntry` никогда не производятся ни одним конструктором в движке; объём (см. «Направление починки») может потребовать заведения отдельной задачи в `ROADMAP.md` при взятии в работу, но сейчас такая задача не заведена — запись остаётся обычным `OPEN`, не `ДОРАБОТКА`.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31)
**Область:** js (`crates/js/src/shim/web_api_shim_tail.js` — `_PERF_SUPPORTED_ENTRY_TYPES`, комментарий на строке 10)
**Владелец:** P3 (оценка объёма при взятии в работу — возможно, нужна дорожка, не один баг).

## Симптом

`_PERF_SUPPORTED_ENTRY_TYPES` (`web_api_shim_tail.js:14`) не включает
`'longtask'` и `'long-animation-frame'`; собственный комментарий строки 10
прямо объясняет почему: «'longtask'/'soft-navigation' are intentionally
excluded — no PerformanceEntry [of either type] is [ever] produced».
`PerformanceObserver.prototype.observe()` с формой единственного `type`
согласно Performance Observer §2.3 шаг 6 при неподдерживаемом типе просто
тихо возвращается (без throw) — печатает предупреждение и ничего не
подписывает. Тест, ожидающий колбэк с записью такого типа, ждёт события,
которое структурно не может произойти.

## Прямое измерение

```
crates/js/src/shim/web_api_shim_tail.js:10: // 'longtask'/'soft-navigation' are intentionally excluded — no PerformanceEntry
crates/js/src/shim/web_api_shim_tail.js:14: var _PERF_SUPPORTED_ENTRY_TYPES = ['largest-contentful-paint', 'layout-shift', …]
```
`longtask`/`long-animation-frame` отсутствуют в списке.

## Кого это держит

- `longtask-timing/supported-longtask-types.window.html`
- `long-animation-frame/loaf-toJSON.html`

## Направление починки

Это не одна строка: производство `PerformanceEntry` типа `longtask`
требует измерения фактической длительности задачи главного потока (порог
50мс по Long Tasks API) в местах, где движок уже гоняет колбэки/таймеры/
скрипты, и заведения нового конструктора записи; `long-animation-frame`
(его преемник, Long Animation Frame API) требует того же плюс разбивку
кадра на фазы (`renderStart`/`styleAndLayoutStart`/scripts). Объём ближе к
задаче в `ROADMAP.md`, чем к точечному багу — при взятии в работу решить,
заводить ли `PERF-`-дорожку, а не чинить список поддерживаемых типов в
одиночку (список уже верен относительно того, что производится).

## Закрытие (2026-09-25, P1)

Закрыт вместе с задачей LONGTASK-1 (`ROADMAP.md`), которая реализовала
модель измерения целиком: срез 1 — `PerformanceLongTaskTiming`/
`PerformanceLongAnimationFrameTiming` подключены, оба типа в
`_PERF_SUPPORTED_ENTRY_TYPES`, `longtask` из `PersistentJs::eval_js` и
парсерных скриптов (`crates/shell/src/scripts.rs`), `long-animation-frame` из
`relayout()`; срез 3 — `scripts[]` из реальных колбэков (события, таймеры,
rAF); срезы 4-5 — culprit-атрибуция источника (`sourceURL`/
`sourceFunctionName`/`sourceLine`/`sourceColumn`/`sourceCharPosition`,
`crates/js/src/v8_runtime/script_attribution.rs`; `sourceCharPosition` —
локальный C++-биндинг `v8::Function::GetScriptStartPosition()` в
`crates/js/cpp/undetectable.cc`).
