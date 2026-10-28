# BUG-907 — preload не экономит запрос и стартует поздно: тела не кэшируются, fetch уходит только после разбора DOM

**Статус:** FIXED 2026-10-28 (P3)
**Заведён:** 2026-08-25 (P1, при починке [BUG-826](BUG-826-FIXED.md) — отделено как остаток)
**Область:** `crates/js/src/dom.rs` (`_lumen_link_hint_fetch` — тело только дренируется и выбрасывается; `_lumen_link_hints_scan` вызывается из `_lumen_apply_ready_state('interactive')`), `crates/shell/src/main.rs` (`dispatch_preload_hints` — сетевого запроса по-прежнему нет), `crates/engine/html-parser/src/preload_scanner.rs` (хинт находится в потоке байт, но им никто не пользуется для fetch-а)
**Владелец:** P1/P3

## Симптом

После починки BUG-826 хинт наконец приводит к запросу и к событию, но не даёт
того, ради чего preload существует:

1. **Тело не переиспользуется.** `<link rel=preload as=script href=x.js>` и
   следующий за ним `<script src=x.js>` — два запроса за один и тот же URL.
   HTML LS §4.6.7 требует обратного: ответ кладётся в preload-кэш и
   последующая настоящая выборка забирает его оттуда. Единственное исключение,
   сделанное в BUG-826, — `modulepreload` с JS-типизированным телом: оно
   регистрируется в module map, поэтому позднейший `import` того же URL
   действительно не качает повторно.
2. **Старт поздний.** Запрос уходит из JS-шима на `readyState='interactive'`,
   то есть **после** разбора документа, тогда как `PreloadScanner`
   (`preload_scanner.rs`) видит хинт ещё в потоке байт из сети. Параллелизм,
   ради которого сканер писался, не получен; на реальном сайте выигрыш
   preload-а сводится к нулю (запрос стартует не сильно раньше, чем сам
   `<script src>`).

## Почему сделано именно так

BUG-826 положил fetch в JS-шим, а не в шелл, сознательно и по прецеденту
`rel=stylesheet` (BUG-703/BUG-722): `load`/`error` принадлежат элементу, а у
шелла нет по-узлового сигнала завершения, который он мог бы переслать на
JS-сторону. Цена — ровно этот баг. Правильное направление — общий кэш ответов
(шелл греет процесс-глобальный prefetch-кэш из streaming-сканера, как он это
уже делает для `PreloadHint::Stylesheet`/`Script` в
`spawn_early_preload_fetches`), плюс путь, по которому JS-сторона узнаёт «этот
URL уже дома» вместо второго запроса.

## Как проверить фикс

WPT `preload/modulepreload.html` и `preload/dynamic-adding-preload.html` считают
записи `performance.getEntriesByName` и проверяют, что скачиваний ровно одно.
Учесть: до этого они упрутся в пустой Resource Timing
([BUG-839](BUG-839-FIXED.md)) — то есть без него измерять придётся на своём
http-сервере, считая (а не дедуплицируя) полученные запросы.

## Закрытие (2026-10-28, P3)

Описание выше — на момент заведения (2026-08-25) и к закрытию устарело: оба
пункта симптома уже были сняты по частям другими задачами.

- **Тело переиспользуется.** BUG-1116: `_lumen_link_hint_fetch` ходит в
  `fetch_preload_cached` (`PREFETCH_CACHE`), а `<script src>`, `<link
  rel=stylesheet>`, `<img>`, `@font-face`, динамический `import()` читают тот же
  кэш (`fetch_preloaded` / `lookup_current`). BUG-1151: `fetch()` забирает
  `as=fetch`-хинт. BUG-1714: хинт-fetch идёт с рабочего потока.
- **Старт ранний.** `warm_preload_cache` (BUG-1116) греет `PREFETCH_CACHE` из
  streaming-сканера, пока HTML ещё приходит; LOAD-3C ограничил пул прогрева ≤ 8.
  Link-заголовок — BUG-906.
- **Замер** (локальный http-сервер, счёт запросов на URL, живое окно
  `--maximized`): `preload as=script/style/image/font/fetch` + `modulepreload` +
  `prefetch`, динамически вставленные `<link rel=preload>` + `<script src>`,
  `new Image()`, `import()` — по одному запросу на URL, все хинты стартуют в
  момент ответа документа. Исключение — `fetch()` из инлайн-скрипта, исполненного
  **до** прохода «interactive»: реестр `as=fetch`-хинтов заполнялся только этим
  проходом, и запрос уходил в сеть второй раз.

Починено именно оно: `_lumen_fetch_take_hint` вызывает
`_lumen_link_fetch_hints_prescan` (`web_api_shim_mid.js`), пока проход по
документу не выполнен, — те же `media`/`type`/`crossorigin`-условия, что у
`_lumen_link_preload`. Тесты: `early_fetch_takes_hint_before_document_pass`,
`early_fetch_ignores_hint_without_crossorigin_or_with_other_media`
(`crates/js/src/dom/tests/v8_formdata.rs`).

## Остаток

- `performance.getEntriesByName` по этим запросам считается WPT-тестами
  `preload/modulepreload.html` и `preload/dynamic-adding-preload.html`; в этой
  задаче они не прогонялись (скачивания считались на своём сервере).
