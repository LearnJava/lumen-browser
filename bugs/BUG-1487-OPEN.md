# BUG-1487 — прогрев `preload`/`modulepreload`/`prefetch` идёт в обход CSP

**Статус:** OPEN
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1185](BUG-1185-FIXED.md); по коду, живьём не снят).
**Область:** shell (`crates/shell/src/page_pipeline.rs:260` `warm_preload_cache` — зовётся из
`page_load.rs` `feed_preload_and_emit` и из `parse_and_layout`; политик не получает).

## Симптом

BUG-1185 закрыл ранний прогрев `<script src>`/`<link rel=stylesheet>` гейтом CSP. Соседний
прогрев author-хинтов — `<link rel=preload as=…>`, `modulepreload`, `prefetch` (BUG-1116) —
по-прежнему шлёт запрос, не глядя на политику: под `script-src 'none'`
`<link rel=preload as=script href=/x.js>` даст `GET /x.js`. CSP3 §4.1.2 запрещает сам запрос;
Chrome его не шлёт.

## Что сделать

Передать в `warm_preload_cache` политики (заголовок ответа + `<meta>` из
`PreloadScanner::meta_csp`, в `parse_and_layout` — `document_csp_policy`) и проверять хинт по
директиве его назначения: `as=script`/`modulepreload` → `script-src-elem`, `as=style` →
`style-src-elem`, `image` → `img-src`, `font` → `font-src`, `audio|video|track` → `media-src`,
`fetch` → `connect-src`, `prefetch` → `default-src` (CSP3 §6.8.1). Критерий: свой сервер не
видит запроса к запрещённому источнику.
