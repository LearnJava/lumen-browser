# BUG-308: страница с HTTP-ошибкой (403) держит document_ready 2–3.5 минуты

**Статус:** FIXED 2026-07-20
**Дата:** 2026-07-17
**Компонент:** shell (навигация/готовность) или network (ретраи?)
**Найден:** живой перф-аудит `/lumen-perf-audit` на www.w3.org (антибот-403)

## Симптом

Навигация на URL, отвечающий HTTP 403, достигает `document_ready`
(MCP `wait{condition:document_ready}`) только через минуты:

```
прогон 1 (без --maximized): w3 ready=205.3 с
прогон 2 (--maximized):     w3 ready=128.9 с
stderr: Ошибка загрузки https://www.w3.org/: network error: HTTP 403
```

Headless-путь (`--dump-source`) тот же 403 отдаёт за 0.75 с — т.е. сеть
отвечает мгновенно, а живое окно после сетевой ошибки минутами не считает
документ готовым (ретраи навигации? условие готовности не выставляется на
error-странице и спасает только вторичный таймер?).

## Влияние

Любой сайт за антиботом/файрволом подвешивает вкладку на минуты вместо
мгновенной страницы ошибки — пользователь видит вечную загрузку.

## Ожидание

Сетевая ошибка навигации → страница ошибки и `document_ready` за долю
секунды (как 0.75 с headless-пути).

## Воспроизведение

```bash
python scripts/perf_audit.py --only w3 --timeout 240
# либо вручную: lumen --mcp-live-port N about:blank + navigate https://www.w3.org/
```

## Корень

Живое окно (`crates/shell/src/main.rs`) обрабатывает провал навигации в двух
ветках — `LoadEvent::LoadError` (сетевой сбой / HTTP-ошибка до рендера) и
`LoadEvent::RenderDone(Err(..))` (сбой финального рендера) — но НИ ОДНА из них
не выставляла никакого сигнала готовности документа. `check_wait_condition`
для `DocumentReady`/`NetworkIdle` резолвится только по двум путям:

- есть JS-контекст → `document.readyState == "complete"`;
- нет JS-контекста → фолбэк `layout_box.is_some()`.

Штатный сценарий репро — навигация из `about:blank` (у которого нет
JS-runtime и нет `layout_box`) на URL, отвечающий ошибкой. После `LoadError`
JS-контекст так и не создаётся, `layout_box` не строится → обе ветки навсегда
`false`, и `wait{document_ready}` висит до собственного дедлайна клиента
(наблюдавшиеся 128–205 с = таймаут MCP-wait, а не ретраи). Headless-путь
(`--dump-source`) не ждёт готовности — он сразу печатает тело ответа, поэтому
там задержки нет.

## Решение

Per-tab-флаг `load_failed: bool` (в активной структуре `Lumen` и в
`PageSnapshot`, чтобы переживать переключение вкладок):

- `true` в обеих error-ветках (`LoadError`, `RenderDone(Err)`);
- `false` в начале каждой навигации (`reload`, стартовый streaming-load),
  при успешном `apply_loaded_page` и в `reset_to_blank`;
- `check_wait_condition(DocumentReady|NetworkIdle)` в самом начале:
  `if self.nav_start.is_none() && self.load_failed { return true; }` —
  осевшая ошибка навигации ЕСТЬ «загрузка завершена». Гейт `nav_start.is_none()`
  исключает выигрыш гонки устаревшим флагом от вытесненной навигации.

## Проверка

Live-MCP репро на connection-refused URL (`http://127.0.0.1:9/`, детерминирован,
без внешней сети): `wait{document_ready}` теперь Ack за ~2 с (был таймаут 20 с).
Репро-скрипт разовый — в коммит не входит.
