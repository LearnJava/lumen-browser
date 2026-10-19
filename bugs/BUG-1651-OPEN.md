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

## Срез 1 (P6, 2026-10-19) — причина найдена, задача 15–18 с → ≤ 2 с

**Стенд.** `scripts/probe_yt_stall.py <lumen.exe> <сек> <url> <тег>` (копия `.tmp/yt_probe.py`): запускает
`--mcp-live-port`, открывает страницу, каждые 3 с шлёт `eval` и пишет `.tmp/yt.<тег>.stderr.log`.
Без прокси youtube с этой машины не открывается (TLS handshake) — запуск с
`LUMEN_PROBE_ARGS="--proxy http://127.0.0.1:12334"`. Нужны `LUMEN_JS_STALL_SAMPLE_MS=500`, `LUMEN_FRAME_LOG=1`
(скрипт ставит сам), для разбивки — `LUMEN_PROFILE_TREE=1 LUMEN_PROFILE_DETAIL=1`. Воспроизводится стабильно:
одна задача `about_to_wait.rs:287` 15–18 с.

**Причина.** Не pump и не таймер: JS плеера youtube (`getPlayerSize` → `element.clientWidth/clientHeight`) в одной
задаче вызывает ≈ 40 принудительных `maybe_flush`, между ними DOM меняется → каждый флаш `path=incremental`, но каждый
пересчитывал 1300–2000 элементов (это путь BUG-935). Три независимые причины тяжёлого флаша:
1. `cs_init` ≈ 80 мкс/узел — `compute_style_shareable` обходил всю унаследованную карту custom properties (youtube
   ≈ 1200–1800 имён) при непустом реестре `@property`. Теперь обход реестра (`style/cascade.rs`): 102 → 6 мс на флаш.
2. Сравнение `prev == style` в `counters.rs::walk` делало глубокое `HashMap == HashMap` по `custom_props` (≈ 1200 строк):
   карта у узла другой `Arc` при том же содержимом (узел, объявивший `--x`, копирует унаследованную карту; потомки наследуют
   копию). Теперь после `share_cache.compute` пересчитанный стиль забирает `Arc` карты вытесненного, если содержимое равно
   (`incr` и тот же родитель) — потомки наследуют ptr-равную карту, сравнения идут по указателю.
3. **Главное.** `has_reach_roots` (`style/restyle.rs`): любая запись под `<body>` делала корнем `<head>` и `<html>` (весь
   документ, 1300–2000 элементов): (а) предыдущие сиблинги предков перебирались при любом `:has(+ …)`/`:has(~ …)` в листе, хотя
   достаёт до них только субъект с вперёд-смотрящим аргументом, — `<head>` как сиблинг `<body>` попадал под любой
   `:not(.x):has(…)`; теперь `HasSubject::sibling_reach` по субъектам; (б) `html:has(…)`/`body:has(…)` в позиции ключевого
   компаунда (`HasSubject::local`: последний в селекторе, не вложен, без sibling-комбинатора после) меняет только стиль
   самого элемента — теперь shallow-корень (элемент и дети, глубже — только если стиль сдвинулся), а не глубокий.

**Замер** (тот же стенд, `dev-release`, 70–90 с): задача `about_to_wait.rs:287` 15–18 с → максимум 1,9–2,8 с; `eval` с
`timeout_ms: 5000` — ни одного `engine thread busy`; максимум пересчитанных за флаш 2022 → 134–161; `cascade_walk` на флаш
300–390 → 20–30 мс. Тесты: `node_fanout_tests::a_descendant_has_argument_does_not_reach_previous_siblings_of_ancestors`,
`bug935_attr_local_roots::a_key_position_has_flip_does_not_recascade_the_subtree` (сверка с полным каскадом внутри `drive`).

**Что осталось (задач > 1 с по-прежнему 3–5 за прогон, до ≈ 1,9 с).**
- JS страницы без флашей: `readBytes ← consume ← eval` (<no-script>:22140) 1,5–1,9 с — выяснить, что это за скрипт и
  почему V8 тратит столько (в Chrome такой длинной задачи на главной нет).
- `incr.sheet_delta` ≈ 350 мс на один флаш (вставка большого `<style>` — разбор/индексация листа).
- Шторм принудительных флашей в одной задаче: 37 флашей по 20–50 мс = 0,8–2,2 с; `cs_match` ≈ 65 мкс/узел (CSS 3,2 МБ).
  Если не снижать цену флаша — бюджет/коалесцирование чтений геометрии (`clientWidth`) в одной задаче: отдельное решение.
- `_lumen_deliver_intersection_observers`/`_lumen_deliver_resize_observers` в `_lumen_tick_timers`/rAF — ≈ 0,5–1,1 с (шим).

## Handoff (P6, 2026-10-19, конец сессии) — гейт не закрыт

Код среза 1 готов (counters.rs, restyle.rs, 2 теста), `cargo clippy -p lumen-layout --all-targets -D warnings` чист, layout `has_`/`bug935`/`node_fanout` зелёные.
`scripts/scoped-test.sh` (лог был в `.tmp/scoped.log`) показал падения, **базу `main` не сверяли**:
- `lumen-layout --lib`: stack overflow `grid_trampoline::deep_grid_chain…` — известный BUG-1266 (`--skip grid_trampoline`).
- `lumen-shell --bin lumen`: `chrome_incremental::bug341_s13_hover_flip…` (412/419 reused), `bug341_s20…` (1 rayon fan-out), `bug341_s21…`
  (3 переиндексации листа) — **могут быть от среза** (chrome.html: ~30 custom properties, канонизация `Arc` в `walk`, `:has`) —
  проверить первыми: `git stash`-free способ — собрать `main` в отдельном worktree и прогнать те же 3 теста.
  Ещё в shell: `page_pipeline::bug269…/bug969…`, `page_resources::frame_subresources…` (похоже на окружение/сеть).
- `lumen-paint`: `display_list::text_and_images::*` (y-смещения текста), `snapshot_tests::*`, `hit_test::…` — слой paint срез не трогает, вероятно окружение (шрифты).
- `lumen-js --lib`: `v8_gap_uashadowslot::*` (details), `v8_idle_message_clipboard::…` — сверить с `main`.
Дальше: сверить падения с `main`; если s13/s20/s21 — от среза, найти причину (подозрение: канонизация `custom_props` в `walk` или
shallow-корень `:has` в `has_reach_roots`); затем закрыть гейт, влить в `main`, удалить указатель из `STATUS-P6.md`,
оставив BUG-1651 OPEN с остатком (см. «Что осталось»).
