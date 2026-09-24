# BUG-1116 — `<link rel=preload>` перезапрашивается шимом синхронно, по одному, мимо кэшей движка

**Статус:** OPEN
**Заведён:** 2026-09-23 (P2, разбор последовательной загрузки после прогона top100,
[журнал](../docs/perf/journal.md) §2026-09-23 top100 split). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js:12182` — `_lumen_link_hint_fetch` →
`fetch(url, {_lumenInitiatorType:'link'})`) + shell (`page_pipeline.rs::dispatch_preload_hints` —
подсказка сканера только логируется, `Event::SubresourceHintFound` не запускает загрузку).

## Симптом

Стенд `.tmp/seqlab/` в worktree аудита (`server.py` задерживает каждый ответ на 700 мс и пишет время каждого запроса; `run.py` гонит видимый Chrome 153 и Lumen в окне `--maximized` с холодным HTTP-кэшем). Страница: CSS с `background-image`, 2 sync + async + defer скрипта в `<head>`, 5 `<img>`, скрипт в конце `<body>` с `new Image()` и `fetch('/api')`.

В `<head>` добавлены `<link rel=preload>` на все 14 подресурсов (имитация повторного визита):

| | Последний ответ |
|---|---|
| Chrome 153, без подсказок / с подсказками | 2.8 с / 2.8 с |
| Lumen, без подсказок | 3.8–4.0 с |
| **Lumen, с подсказками** | **14.5 с** — после загрузки страницы 14 повторных запросов строго по одному, 700 мс каждый |

Прогон top100 2026-09-23 (stderr всех 100 сайтов, `Counter` по строкам `→ GET`): **1282 из 6618 GET
(19.4 %) — повторные**, на 53 сайтах; tradingview 204, github 76, tiktok 73, cnbc 53, airbnb 44 —
почти все повторные URL есть среди `⤷ preload`.

## Корень

Две загрузки одного ресурса, не знающие друг о друге:
1. сканер предзагрузки (`dispatch_preload_hints`) только эмитит событие в лог — сам ресурс
   позже грузит парсер/подгрузчик картинок движка;
2. шим, обрабатывая DOM-элемент `<link rel=preload>`, делает собственный `fetch()` (без
   AbortSignal → синхронный путь, см. PERF-14) в `setTimeout(0)`-задаче: запросы идут
   последовательно, блокируют JS-поток на RTT каждый и не попадают ни в `PREFETCH_CACHE`,
   ни в `IMAGE_CACHE`.

## Что сделать

`link rel=preload` должен класть ответ туда, откуда его возьмёт настоящий потребитель
(HTML LS §4.6.7 «preload cache»), и не блокировать JS-поток: загрузка в движке, параллельно,
а шим получает только событие `load`/`error`. Критерий: на стенде вариант «с подсказками» не
медленнее варианта без них, в прогоне top100 повторных GET по preload-URL — 0.

## Срез 1 (2026-09-24)

Для `preload`/`modulepreload`/`prefetch` (все `as`-значения, кроме `image`/`font` —
см. остаток ниже) — общий путь с настоящим потребителем:

- `page_pipeline.rs::warm_preload_cache` спавнит по фоновому потоку на каждый такой
  хинт, как только сканер его увидел (вызывается и из стримингового
  `page_load.rs::feed_preload_and_emit`, и из финального `parse_and_layout`), прогревая
  `PREFETCH_CACHE` тем же путём (`fetch_subresource_with_content_type` +
  `RequestDestination::Prefetch`), каким уже идут реальные `<script src>`/
  `<link rel=stylesheet>`.
- `lumen-core::ext::SubresourceCache` — новый трейт, вынесенный в `lumen-core`, чтобы
  `lumen-network::HttpClient` мог читать `PREFETCH_CACHE` без обратной зависимости
  `network → shell`; `crates/shell/src/prefetch.rs::SharedPrefetchCache` — единственная
  реализация, тонкая обёртка над уже протестированным `PrefetchCache::fetch`.
- `HttpClient::fetch_preload_cached` (новый метод `JsFetchProvider`, дефолт — просто
  `fetch_sync`) читает/пишет через `SubresourceCache`, когда он подключен
  (`with_subresource_cache`, только для документного `HttpClient` в
  `page_pipeline.rs::parse_and_layout`).
- JS-шим (`_lumen_link_hint_fetch` в `web_api_shim_mid.js`) переведён с собственного
  `fetch()` на новый нативный биндинг `_lumen_link_prefetch_sync` →
  `fetch_preload_cached` — тот же `FetchCache`-слот, что у `_lumen_fetch_sync`.
  Статус-код по-прежнему проверяется в JS (`status < 200 || status >= 300` → `error`),
  потому что Rust-сторона намеренно не отличает 2xx от остального (как и у
  `<script>`/`<link rel=stylesheet>` — HTTP-статус решает вызывающий JS, не транспорт).

Живая проверка (стенд `.tmp/`, не сохранён — сервер с искусственной задержкой 0.3с,
страница с 3 `<link rel=preload as=script|style>` на те же URL, что реально грузят
`<script src>`×2 + `<link rel=stylesheet>`×1): до фикса (`main`) — 7 GET (3 дубликата,
все `link-onload` тем не менее срабатывают); после фикса — 4 GET (page + 3 ресурса, без
дублей), все три `link.onload` продолжают срабатывать. `--dump-display-list`/
`--dump-layout` не годятся для такой проверки — они не крутят цикл таймеров
(`setTimeout(0)`, которым живёт `_lumen_link_hint_fetch`), нужен живой `--maximized`.

Гейты: `cargo clippy -p lumen-core -p lumen-network -p lumen-js -p lumen-shell
--all-targets -- -D warnings` чист. `scripts/scoped-test.sh` (main) — два теста
(`icon_link_fires_error_on_http_failure`, `preload_link_fires_error_on_http_failure`)
сначала упали (Rust-сторона отдаёт `status: 200` безусловно, JS больше не проверял код),
починено добавлением проверки статуса в шиме; после фикса оба зелёные в изоляции и в
полном прогоне `v8_webworker`. Остальные 9-11 упавших в разных прогонах
(`worker_*`/`shared_worker_*`/`frame_bridge::inaccessible_bridge_mutation_...`) не
относятся к этой правке (`git diff --stat main -- crates/js/src/worker.rs` пуст) и не
воспроизводятся при повторном изолированном прогоне того же теста — известная флакость
`v8_webworker` под нагрузкой (см. память). `python graphic_tests/dump_golden.py --build`
— 12/12 совпадают с эталоном (эта правка не трогает paint/layout).

**Остаток:** `<link rel=preload as=image>`/`as=font` — тот же `PreloadHint::Preload`,
поэтому `warm_preload_cache` их тоже греет в `PREFETCH_CACHE`, но настоящий потребитель
картинки/шрифта читает через `IMAGE_CACHE`/шрифтовый загрузчик, а не через
`PREFETCH_CACHE` — дубликат для этих двух `as`-значений не устранён (изначальный симптом
бага упоминал оба кэша: «мимо `PREFETCH_CACHE`/`IMAGE_CACHE`»). Прогон top100 с
подсчётом повторных `→ GET` (числа из журнала — 1282/6618, 19.4%) в этом срезе не
повторён; ожидание — заметное снижение (script/stylesheet — самые частые `as` в подсчёте
среди tradingview/github), но не до нуля, пока `as=image`/`as=font` не заведены на общий
кэш с картиночным/шрифтовым загрузчиком. Бага остаётся OPEN.
