# BUG-809 — Layout Instability объявлен, но ни одна запись `layout-shift` не доставляется: шелловский триггер `deliver_layout_shift` не вызывается ниоткуда

**Статус:** OPEN (ДОРАБОТКА → [GAP-LAYOUTSHIFT](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-LAYOUTSHIFT` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 17 — категория `layout-instability`, 35 TIMEOUT из 37 прогнанных, 94.6 %)
**Область:** `crates/shell/src/main.rs:2925` (объявление `deliver_layout_shift` в трейте, помечено `#[allow(dead_code)]`), `crates/shell/src/main.rs:3359` (реализация — зовёт JS-хук), `crates/js/src/dom.rs:11035` (`_lumen_deliver_layout_shift`), `crates/js/src/dom.rs:10907` (`_PERF_SUPPORTED_ENTRY_TYPES`, где `layout-shift` объявлен поддерживаемым)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Страница подписывается на `layout-shift`, двигает блок и ждёт запись —
которая не приходит никогда. Страница при этом жива, исключений нет,
`supportedEntryTypes` уверяет, что тип поддержан:

```js
// layout-instability/simple-block-movement.html, сокращённо
const watcher = new ScoreWatcher;              // не бросает: тип «поддержан»
promise_test(async () => {
  await waitForAnimationFrames(2);
  document.querySelector("#shifter").style = "top: 160px";   // сдвиг 300×200 на 160px
  await watcher.promise;                       // ← висит до таймаута враннера
}, 'Simple block movement.');
```

Ровно это объявление и превращает категорию в TIMEOUT, а не в FAIL:
`ScoreWatcher` (`layout-instability/resources/util.js`) первой строкой
проверяет `PerformanceObserver.supportedEntryTypes.indexOf("layout-shift")`
и бросает `Error("Layout Instability API not supported")`, если типа нет.
Честный ответ «не поддерживаю» дал бы быстрый провал; ложноположительный
даёт зависание. Тот же класс расхождения, что закрытый
[BUG-354](BUG-354-FIXED.md) («геттер обещает пять типов, которых нет»), — но
здесь тип из списка не убрали, потому что доставка *почти* есть.

## Прямое измерение

`tests/wpt/verify_layout_shift_and_peer_gaps.py` (живое окно, http, улики из
stderr браузера; dev-release, Linux, 2026-08-21, коммит `79ea47826`,
`--seconds 8`; 15 тиков `setInterval` — страница жива всё это время):

| проба | получено |
|---|---|
| `cls-feature-detect` | `supported=…,layout-shift,…`, `LayoutShift=undefined`, `LayoutShiftAttribution=undefined`, `observe-ok` |
| `cls-shift` — наблюдаем, ждём 2 кадра, двигаем блок 300×200 на 160px | только `shifted`; **записи нет** |
| `cls-shift-buffered` — сдвиг до создания наблюдателя, `{type, buffered: true}` | только `shifted`; **буфер пуст** |
| `cls-attribution` — читаем `entry.sources[0].node` | только `shifted`; колбэка нет вовсе |

## Причина (локализована чтением кода)

Цепочка доставки построена целиком, но у неё нет входа:

* `_lumen_deliver_layout_shift(value, session_id, had_input)`
  (`crates/js/src/dom.rs:11035`) собирает запись и рассылает наблюдателям —
  рабочий код;
* шелловский `deliver_layout_shift` (`crates/shell/src/main.rs:3359`) зовёт
  этот хук — рабочий код;
* объявление того же метода в трейте (`main.rs:2925`) помечено
  `#[allow(dead_code)]`, и `grep -rn deliver_layout_shift crates/` даёт
  ровно шесть совпадений: объявление, реализация, JS-хук, заглушки в
  `driver`/`winit_session`/`core::ext` — **и ни одного вызова из layout или
  reflow**. Никто не считает сдвиги и никто не зовёт триггер.

Дополнительно отсутствуют `window.LayoutShift` и `LayoutShiftAttribution`
(тип записи не веб-видим), а `sources` в `_lumen_deliver_layout_shift`
захардкожен пустым массивом — то есть даже после включения триггера
`sources.html`/`attribution-*.html` останутся красными, но уже как FAIL.

Всё это было замечено ещё при вендоринге категории
(`WPT-VENDOR-layout-instability`, 2026-08-05: «Rust-триггер помечен
`#[allow(dead_code)]` и нигде не вызывается»), но номера тогда не получило —
заводится сейчас, когда измерена корпусная цена.

## Масштаб

Механизм `layout-shift-never-delivered` в `tests/wpt/timeout_audit.py`
забирает **35 id** остатка снимка WPT-RUN-5 — всю неразобранную часть
категории `layout-instability` (35 из 37 её TIMEOUT; оставшиеся два
объяснены `iframe-no-nested-context` и `helper-404`). Это самая плотная
категория остатка на момент среза 17: 94.6 % её таймаутов — один этот
дефект. Остальные ~29 FAIL категории (по прогону вендоринга) — известные
маски [BUG-384](BUG-384-FIXED.md)/[BUG-525](BUG-525-OPEN.md) и
неимплементированные testdriver-экшены ([BUG-810](BUG-810-OPEN.md)).

Цена шире WPT: CLS — одна из трёх метрик Core Web Vitals, и любая
RUM-библиотека (web-vitals.js и производные) на Lumen сегодня получает
`supportedEntryTypes`, включающий `layout-shift`, подписывается и молча
не получает ничего — то есть считает CLS равным нулю, а не «не измеримо».

## Направление починки (не предписание)

Считать сдвиг там, где вёрстка уже пересчитывается: у релэйаута есть и
старые, и новые прямоугольники, а `deliver_layout_shift` ждёт готовую
дробь. Минимальный полезный шаг — доля площади сдвинувшихся элементов на
нормированное расстояние (Layout Instability §3), с `had_input` из
недавнего пользовательского ввода. Веб-видимые `LayoutShift`/
`LayoutShiftAttribution` и непустой `sources` — отдельный, следующий шаг:
без них тесты перестанут виснуть и станут честно падать.

Альтернатива, если считать сдвиги пока не планируется: убрать
`layout-shift` из `_PERF_SUPPORTED_ENTRY_TYPES` (`dom.rs:10907`). Это не
починка API, но она немедленно превращает 35 зависаний в 35 быстрых
провалов с внятной причиной и снимает ложноположительный feature-detect для
любой RUM-библиотеки — ровно то решение, которое уже принято в BUG-354 для
пяти других типов.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_layout_shift_and_peer_gaps.py
   --variant cls-shift` печатает `cls-entry value=…`.
2. `--variant cls-shift-buffered` печатает `cls-buffered-entries=1` и больше.
3. WPT: `run_report.py --all --root layout-instability --recursive` — 35
   TIMEOUT уходят; часть тестов станет FAIL (пустой `sources`,
   отсутствующий `LayoutShift`), и это ожидаемый промежуточный результат.
