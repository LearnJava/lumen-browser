# BUG-1651 — youtube: engine-поток занят одной задачей таймеров/pump 8–9 с

**Статус:** OPEN
**Заведён:** 2026-10-19 (Chat, проверка youtube после BUG-1239/1633/1634/1638/1639)
**Область:** shell / js (`crates/shell/src/app/about_to_wait.rs:287` — `route_task_js` → `tick_timers`/`pump_*`) — не локализовано

## Симптом

Та же сборка и запуск, что в [BUG-1650](BUG-1650-FIXED.md), главная `https://www.youtube.com/`. Живой `eval`
(`timeout_ms: 5000`) примерно на 50–70-й секунде после открытия дважды подряд отвечает:

```
Eval error: engine thread busy: eval not run within 5.0 s
(running task from crates\shell\src\app\about_to_wait.rs:287:13 for 8.7 s)
```

`about_to_wait.rs:287` — пер-тиковый батч `route_task_js(|j| { j.tick_timers(); j.pump_websockets(); j.pump_sse();
j.pump_media(); j.pump_workers(); … })`. Одна такая задача идёт ≥ 8,7 с: всё это время страница не реагирует
на ввод (тот же поток). В Chrome главная youtube не даёт долгих задач такого масштаба.

## Что сделать

1. Выяснить, что внутри батча занимает секунды: таймер-колбэк страницы (какой скрипт), `pump_workers`
   (ServiceWorker youtube), `pump_media` — замер по вызовам (`LUMEN_FRAME_LOG`/трасса `--trace-nav` или
   временные метки в `.tmp`-сборке).
2. Если это JS страницы — сравнить с Chrome Performance (длинные задачи главной youtube) и найти, что
   у нас медленнее (горячая функция шима, синхронный relayout из JS, GC).
3. Если это наш pump — ограничить работу за тик (бюджет), чтобы ввод и `eval` не ждали секунды.

**Готово, когда:** на главной youtube за первые 90 с нет задачи engine-потока длиннее ~1 с; живой `eval`
с `timeout_ms: 5000` не получает `engine thread busy`.

Связано: [BUG-935](BUG-935-OPEN.md) (долгие задачи `about_to_wait` на rAF-мутациях) — проверить, не тот ли путь.

## Handoff (P6, 2026-10-19) — диагноз сделан, исправление в работе

**Стенд.** `scripts/probe_yt_stall.py <lumen.exe> <сек> <url> <тег>` (копия `.tmp/yt_probe.py`): запускает
`--mcp-live-port`, открывает страницу, каждые 3 с шлёт `eval` и пишет `.tmp/yt.<тег>.stderr.log`.
Без прокси youtube с этой машины не открывается (TLS handshake) — запуск с
`LUMEN_PROBE_ARGS="--proxy http://127.0.0.1:12334"`. Нужны `LUMEN_JS_STALL_SAMPLE_MS=500`, `LUMEN_FRAME_LOG=1`
(скрипт ставит сам), для разбивки — `LUMEN_PROFILE_TREE=1 LUMEN_PROFILE_DETAIL=1`. Воспроизводится стабильно:
одна задача `about_to_wait.rs:287` 15–18 с.

**Причина.** Не pump и не таймер: JS плеера youtube (`getPlayerSize` → `element.clientWidth/clientHeight`)
в одной задаче вызывает ≈44 принудительных `maybe_flush` (по 0,3–1 с), между ними DOM меняется (`style`/`class`/
ChildList у `ytd-*`) → каждый флаш `path=incremental`, но с глубокими корнями и **~1300 пересчитанных элементов**
(`forced_same` ≈ 96 %). Это путь BUG-935 (флаши под JS-циклом). Расклад `cascade_walk` ≈ 300–390 мс на флаш:
1. `cs_init` ≈ 80 мкс/узел — `compute_style_shareable` обходил **всю** унаследованную карту custom properties
   (youtube: ≈1200–1800 имён) при непустом реестре `@property` (там 1 правило). **Исправлено** в этой ветке
   (`style/cascade.rs`: обход реестра вместо карты): `cs_init` 102 → 6 мс на флаш.
2. `w_eq` ≈ 150–175 мс из ~300 — **главный остаток**: в `counters.rs::walk` (ветка `(None, None)`) сравнение
   `**prev == *style` (и `*prev != *style` в shallow-ветке) делает глубокое `HashMap == HashMap` для
   `custom_props` (≈1200 строк), потому что карта у узла другой `Arc`, хотя содержимое то же (`CustomProps::eq`
   → `ptr_eq || ==`). Замерено временными `scope_detail("w_share"/"w_write"/"w_eq"/"w_repl")` (в коммит не вошли).
3. `cs_match` ≈ 85 мс/1280 узлов (≈65 мкс/узел; youtube CSS 3,2 МБ) и `w_share` сверх `compute_style` ≈ 28 мкс/узел
   (Arc::new большого `ComputedStyle`).

**План (следующая сессия).**
- В `counters.rs`: пер-проходная память сравнений `PropsMemo` в `CounterMap` (ключ — пара указателей двух
  `CustomProps`, значение — keep-alive клоны + результат), после `share_cache.compute` — канонизация: если у
  вытесненного (`prev`) стиля `custom_props` не ptr-равна, но равна по содержимому — подставить `prev`-овый `Arc`
  (`Arc::make_mut(&mut style).custom_props = prev`), чтобы потомки наследовали ptr-равную карту; сравнения
  `prev == style` делать как `memo.equal(prev.custom_props, style.custom_props) && **prev == *style`.
  Нужен `CascadeStyles::prev_custom_props(id)` (без перештамповки). Это снимает ≈ половину флаша.
- Затем смотреть `cs_match` (индекс правил под 3,2 МБ CSS) и число пересчитываемых при `style`-записи на `ytd-*`.
- Цель «Готово, когда»: за 90 с нет задачи > ~1 с; при 44 флашах за задачу нужно ≲ 20 мс на флаш — одной
  канонизацией не достичь; если после п.1–2 задача остаётся > 5 с, рассмотреть бюджет/коалесцирование
  принудительных флашей чтения геометрии (`clientWidth` при ≥N флашей подряд в одной задаче) — отдельным решением.
- Проверка: тот же стенд, `grep "\[engine\] task [0-9]{4,}"` и `w_eq`/`cascade_walk` в профиле; `cargo test -p lumen-layout`
  (restyle/counters/cascade), clippy `-D warnings`, затем `graphic_tests` не нужен (пиксели не двигаются) —
  `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.
