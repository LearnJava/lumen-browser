# BUG-678 — Soft Navigation detection never fires: `_lumen_deliver_soft_nav` has no caller anywhere in the engine

**Статус:** OPEN (ДОРАБОТКА → GAP-SOFTNAV)
**Компонент:** js (`crates/js/src/soft_navigation.rs` — the shim; missing caller in `crates/shell/src/main.rs`/`crates/driver/src/session.rs`, cf. how `_lumen_deliver_lcp_entry`/`_lumen_deliver_paint_entry` are wired at `crates/shell/src/main.rs:3232-3244` and `crates/driver/src/session.rs:1316`)
**Найден:** P2, WPT-VENDOR-soft-navigation-heuristics, 2026-08-06

## Симптом

Категория `soft-navigation-heuristics` (`tests/wpt/soft-navigation-heuristics/`,
104 файла, 94 отобранных id) — вендорена и прогнана целиком (`run_report.py
--all --root soft-navigation-heuristics --recursive --processes=4`, 1:38) —
**73/94 harness OK, 2/101 сабтестов**. Необычно высокий harness-OK для
testdriver-насыщенной категории (93/103 файлов упоминают `testdriver`, но
`SKIP` не было ни одного) — причина в том, что вся категория опирается
только на `test_driver.click()`, единственное реально реализованное действие
исполнителя (см. `executors/executorlumen.py::_handle_action`).

Доминирующий сигнал — 54 FAIL с `Error: Timed out waiting for LCP entries`
(тестовый хелпер `resources/soft-navigation-test-helper.js` ждёт запись
`largest-contentful-paint`/`soft-navigation` через `PerformanceObserver`
после клика + `history.pushState`/Navigation API навигации). Она никогда не
приходит: `_lumen_deliver_soft_nav` (`soft_navigation.rs:63`) — единственная
точка, кладущая `PerformanceSoftNavigationEntry` в `performance._perf_entries`
и уведомляющая наблюдателей — не вызывается ни из одного места в движке:

```
grep -rn "_lumen_deliver_soft_nav" crates/ --include=*.rs
# только определение в soft_navigation.rs — ни одного вызова
```

Для сравнения, соседние perf-хуки того же семейства реально подключены к
рендер-пайплайну:

```
crates/driver/src/session.rs:1316:  "_lumen_deliver_lcp_entry({}, {}, {}, {})"
crates/shell/src/main.rs:3232:      "_lumen_deliver_paint_entry({}, {start_ms})"
crates/shell/src/main.rs:3244:      "_lumen_deliver_lcp_entry({element_id}, {size}, {start_ms}, {render_time_ms})"
```

`_lumen_deliver_soft_nav` не встречается ни в `shell/`, ни в `driver/` вовсе —
ничто в движке не детектирует «клик + URL-изменение без полной перезагрузки»
и не сбрасывает LCP-кандидатов для нового «мягкого» замера. Это отличается от
уже задокументированного [BUG-354](BUG-354-OPEN.md): тот баг про то, что
`supportedEntryTypes` **лжёт**, включая `soft-navigation` в список
поддерживаемых типов. Здесь — независимая находка на уровне поведения:
даже код, что не полагается на feature-detection и просто пробует
подписаться на `soft-navigation`/наблюдать LCP после клика, никогда не
получит колбэк, потому что событие детекции в принципе не генерируется.

Второстепенный сигнал (14 FAIL) — `assert_implements: LargestContentfulPaint
is not implemented`: `window.LargestContentfulPaint` (WebIDL-конструктор
записи) отсутствует вовсе, хотя `largest-contentful-paint` записи реально
доставляются (`dom.rs:8375-8389`) как безымянные объектные литералы, не
инстансы какого-либо класса — тот же класс дефекта, что BUG-673
(`PerformanceResourceTiming`/`PerformanceNavigationTiming`), здесь впервые
конкретно для LCP.

Прочий сигнал — реконфирмация уже открытых дефектов, не новых номеров:
34 FAIL `TypeError: elementDocument.contains is not a function`
([BUG-574](BUG-574-OPEN.md), `Node.contains` отсутствует), 28 FAIL/ERROR
`ReferenceError: SoftNavigationTestHelper is not defined` — 404 на
`../../resources/soft-navigation-test-helper.js` из `dom/tentative/`/
`detection/tentative/` подкатегорий ([BUG-346](BUG-346-OPEN.md), `..`-сегменты
в `Url::resolve()` не коллапсируются; абсолютный путь
`/soft-navigation-heuristics/resources/...` резолвится корректно).

## Причина

`install_soft_navigation_api_v8` (`soft_navigation.rs:20-24`) устанавливает
только класс `PerformanceSoftNavigationEntry` и функцию-хук
`_lumen_deliver_soft_nav` — доку-комментарий модуля прямо называет её
«shell hook», подразумевая, что shell должен вызывать её при обнаружении
мягкой навигации. Детектирующая часть (перехват `history.pushState`/
`history.replaceState`/Navigation API `navigate()` в окне активной
пользовательской активации и последующий повторный замер LCP) никогда не
была реализована ни в `crates/shell/`, ни в `crates/driver/`, ни где-либо
ещё в движке — модуль на Phase 0 дальше самого класса не продвинулся,
в отличие от соседних `_lumen_deliver_lcp_entry`/`_lumen_deliver_paint_entry`,
у которых реальный вызывающий код есть.

## Масштаб

Вся категория Soft Navigation Heuristics (94 отобранных id) не может дать ни
одного зелёного результата за пределами структурных проверок — 73 «harness
OK» здесь означает «страница не зависла», а не «фича работает»: 0/101
содержательных сабтестов, кроме двух краевых случаев, не проверяющих сам
факт детекции.

## Дальше

Fix scope: подключить реальный вызов `_lumen_deliver_soft_nav(url, startTime,
durationMs)` из shell/driver в точке, где обнаружено (a) `pushState`/
`replaceState`/Navigation API-навигация, (b) вызванная в рамках активного
пользовательского взаимодействия (клик/keydown в пределах spec-определённого
окна), и (c) сопровождающаяся DOM-изменением — по образцу существующей
проводки `_lumen_deliver_lcp_entry`/`_lumen_deliver_paint_entry`. Затем
повторно завести LCP-наблюдение (сброс текущего «наибольшего» кандидата) на
момент детекции, чтобы `getLargestInteractionContentfulPaint()`-класс тестов
тоже мог пройти. `LargestContentfulPaint`-конструктор — отдельная, меньшая
правка в `dom.rs` (аналогично шаблону `Symbol.toStringTag`/WebIDL-обёртки из
[BUG-677](BUG-677-OPEN.md)/[BUG-673](BUG-673-FIXED.md)).

## Ревизия P3 2026-09-28: дефекты закрыты, остаток — ДОРАБОТКА → GAP-SOFTNAV

Взят как следующий top-down пункт `STATUS-P3.md` (BUGS.md:72; строки выше —
DEBTOR-якоря ратчетов с остатком в доменах P1 и приостановленный пользователем
BUG-341). Запись смешивала два разных по природе наблюдения — их развёл по
правилу `docs/probe-method.md` §8.

**Исправлено (дефекты реализованного кода):**

1. **`window.LargestContentfulPaint` отсутствовал.** Заведён по образцу
   `PerformancePaintTiming` (BUG-645): `new` бросает `TypeError`, записи,
   которые доставляет `_lumen_deliver_lcp_entry`, строятся через
   `Object.create(LargestContentfulPaint.prototype)`, `[Default] toJSON`
   сериализует IDL-атрибуты (LCP §2). Попутно добавлены недостававшие
   атрибуты `renderTime`/`loadTime` (`render_time_ms`/`start_ms` хука).
   Код — `crates/js/src/shim/web_api_shim_tail.js`, экспорт на `window` —
   `web_api_shim_tail_mc.js`. Тест —
   `dom::tests::v8_perf_observers::largest_contentful_paint_interface_backs_lcp_entries`.
2. **Хук `_lumen_deliver_soft_nav` писал в пустоту.** Он клал запись в
   `performance._perf_entries` и звал `performance._observers[i]._callback` —
   ни того, ни другого в шиме нет (буфер — глобальный `_perf_entries`,
   наблюдатели — `_perf_observers` с доставкой задачей §5.3), поэтому даже
   подключённый вызов не дал бы ни `getEntriesByType('soft-navigation')`, ни
   колбэка. Теперь хук идёт тем же путём, что все остальные типы записей:
   `_perf_entries.push` + `_perf_observer_notify`. Юнит-тесты модуля, которые
   подменяли `performance` фальшивым объектом и тем самым проверяли
   несуществующий контракт, заменены тестом с настоящим шимом —
   `dom::tests::v8_perf_observers::soft_nav_hook_feeds_the_performance_timeline`.

Не тронуто сознательно: `'soft-navigation'` по-прежнему вне
`_PERF_SUPPORTED_ENTRY_TYPES` (правило BUG-354 — тип объявляется
поддерживаемым только когда что-то реально производит записи), поэтому
`observe({type: 'soft-navigation'})` до детекции ничего не получит.
`duration` LCP-записи по-прежнему `render_time_ms - start_ms`, хотя LCP §2
требует 0; живой shell LCP-записей не доставляет вовсе
(`PersistentJs::deliver_lcp_entry` помечен `#[allow(dead_code)]`, вызывает его
только `lumen-driver`) — это часть той же доработки, не точечная правка.

**Переквалифицировано (ДОРАБОТКА → GAP-SOFTNAV).** Оба условия §8 выполнены:

1. *Функциональности нет вовсе:* `grep -rn "_lumen_deliver_soft_nav" crates/`
   даёт только определение и тесты — ни shell, ни driver мягкую навигацию не
   детектируют.
2. *Объём — модель состояния:* Soft Navigations требует атрибуции задач
   пользовательского взаимодействия (клик/keydown → обработчики → промисы/
   таймеры, порождённые ими), фиксации изменения URL (`pushState`/
   `replaceState`/Navigation API) и DOM-модификации в пределах той же
   атрибуции, затем contentful paint новых узлов, `navigationId` на всех
   последующих записях и повторного замера LCP (сброс кандидата) — плюс живая
   доставка LCP из shell, которой нет. Точечной правки в одном месте нет.

Статус — `OPEN (ДОРАБОТКА → GAP-SOFTNAV)`, строка снята с `STATUS-P3.md`.
