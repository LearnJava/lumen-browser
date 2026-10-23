# BUG-1491 — прогрев `preload`/`modulepreload`/`prefetch` идёт в обход CSP

**Статус:** FIXED 2026-10-23 (P3)
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

## Исправление

Прогрев и собственный fetch элемента проходят один предикат —
`CspPolicy::preload_fetch_allows(destination, url, origin, nonce)` в
`lumen-network::csp`: `script`/`modulepreload` → `script-src-elem` (nonce хинта),
`style` → `style-src-elem` (nonce), `image`/`icon` → `img-src`, `font` → `font-src`,
`audio|video|track` → `media-src`, `fetch|json` → `connect-src`,
`worker|sharedworker` → `worker-src` (через `child-src`), `prefetch` →
`prefetch-src` с откатом на `default-src`; прочие `as` не гейтятся.

- Ранний прогрев: `csp_enforce::speculative_fetch_blocked` теперь покрывает
  `Preload`/`ModulePreload`/`Prefetch`; `warm_preload_cache` получает политики
  документа (заголовок + `<meta>`). В `feed_preload_and_emit` — `header_csp` +
  `scanner.meta_csp()`, в `parse_and_layout` — `csp_header` + `<meta>` из нового
  `scan_preload_hints_with_meta_csp` (DOM ещё нет).
- Собственный fetch `<link>` в JS-шиме (`_lumen_link_hint_fetch` →
  `_lumen_link_prefetch_sync(url, as, nonce)` → `JsFetchProvider::fetch_preload_cached`)
  идёт через `HttpClient::preload_src_gate` до кэша и сети; запрещённый хинт
  получает `error`, запроса нет. Без этого шим сам слал бы тот же `GET`, что и
  пропущенный прогрев. `rel=icon` шим теперь помечает `as=icon` (→ `img-src`).
- `PreloadHint::Preload`/`ModulePreload` несут `nonce` элемента.

Тесты: `preload_hint_is_judged_by_its_destination_directive`,
`preload_hint_ignores_other_directives_and_honours_nonce` (network),
`speculative_fetch_blocked_covers_preload_hints` (shell),
`warm_preload_cache_sends_no_request_forbidden_by_csp` (свой TCP-сервер считает
пути), `preload_hints_carry_nonce_and_meta_csp_is_collected` (html-parser).

Остаток: событие `securitypolicyviolation` для запрещённого `<link rel=preload>`
не диспатчится (элемент получает только `error`).
