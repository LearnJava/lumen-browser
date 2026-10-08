# BUG-1185 — ранняя предзагрузка запрашивает `<script src>`/`<link>`, запрещённые CSP

**Статус:** FIXED 2026-10-08 (P3; код написан 2026-09-26, довлит на свежий main)
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1183](BUG-1183-FIXED.md); свидетель — свой сервер).
**Область:** shell (`crates/shell/src/page_load.rs:2599` `feed_preload_and_emit` — ранний
прогрев stylesheet/script из сканера потокового разбора; CSP заголовка ответа в нём не читается).

## Симптом

Сервер отдаёт страницу с CSP в заголовке ответа и считает запросы.

| Страница, политика | Lumen dev-release | Chrome 153 |
|---|---|---|
| `script-src 'none'`, `<script src=/s2.js>` | `GET /s2.js` | запроса нет |
| `style-src 'none'`, `<link rel=stylesheet href=/red2.css>` | `GET /red2.css` | запроса нет |

Файл скачивается, но не исполняется и не применяется: окончательные гейты (`resolve_script_sources`,
`load_linked_stylesheets`) блокируют его и шлют `securitypolicyviolation`. В логе запрос идёт
раньше или сразу после строки `⤷ preload js|css`. Лишний запрос — утечка: CSP запрещает
именно обращение к источнику, не только его исполнение (CSP3 §4.1.2 «Should request be blocked»).
Так же ведут себя `*-src-elem` после [BUG-1183](BUG-1183-FIXED.md).

## Репро

Сервер `target/bug1183/server.py` (worktree `p3-work`), страницы `/p8.html`, `/p9.html`; счёт
запросов в `target/bug1183/srv.log`.

## Что сделать

Проверять CSP заголовка ответа (`script_element_fetch_allows`/`style_element_fetch_allows`) до
раннего прогрева; `<meta>`-CSP к этому моменту может быть ещё не разобран — тогда прогрев из-под
документа с `<meta http-equiv=Content-Security-Policy>` лучше не делать вовсе.
Критерий: на обеих страницах запроса нет, как у Chrome.

## Решение (2026-10-08, P3)

Ранний прогрев в `feed_preload_and_emit` (`crates/shell/src/page_load.rs`) теперь пропускает
`<script src>`/`<link rel=stylesheet>`, которые запрещает хоть одна политика документа, —
`csp_enforce::speculative_fetch_blocked` спрашивает те же элементные проверки, что окончательные
гейты (`script_element_fetch_allows` / `style_element_fetch_allows`), с `nonce`/`integrity`
из самого hint-а. Политики двух видов:

- **заголовок ответа.** Сетевой sink потокового тела (`ChunkSink`, `PageChunkSink`,
  h3 `BodySink`) передаёт с каждой порцией заголовки того ответа, чьё тело течёт: для
  HTTP/1.1 и h3 — сами заголовки, для кэша — сохранённые, для перехватчика Service Worker —
  пустой срез. После редиректа это заголовки финального ответа, не 3xx. Shell разбирает CSP
  один раз на hop.
- **`<meta http-equiv=Content-Security-Policy>`.** `PreloadScanner` копит их по мере потока
  (`PreloadScanner::meta_csp`). Политика из того же chunk-а применяется и к hint-ам до неё —
  строже спецификации, но цена ошибки — только пропущенный прогрев: окончательный гейт решает
  сам.

`PreloadHint::Script` несёт `nonce` и `integrity`, `PreloadHint::Stylesheet` — `nonce`, чтобы
прогрев под nonce- и хэш-политиками не выключался целиком.

Тесты: `preload_scanner::streaming_tests::script_and_stylesheet_carry_nonce_and_integrity`,
`preload_scanner::streaming_tests::scanner_collects_meta_csp_across_chunks`
(lumen-html-parser); `tests::fetch_page_streaming_chunks_carry_final_response_headers`
(lumen-network); `csp_enforce::tests::speculative_fetch_blocked_follows_element_directives`,
`csp_enforce::tests::speculative_fetch_blocked_honours_hint_nonce` (lumen-shell).

### Живая проба

Сервер `target/bug1183/server.py`, Lumen dev-release `--maximized`, `LUMEN_NO_ADBLOCK=1`,
`target/bug1183/lumen_probe.py`; счёт запросов — лог сервера.

| Страница, политика | Запросы Lumen после правки |
|---|---|
| p8 `script-src 'none'` (заголовок), `<script src=/s2.js>` + `<link href=/nope.css>` | `/nope.css` (стили политика не ограничивает); `/s2.js` нет |
| p9 `style-src 'none'` (заголовок), `<link href=/red2.css>` | `/red2.css` нет |
| p10 `<meta>` `script-src 'none'; style-src 'none'`, `/s3.js` + `/red3.css` | ни одного |
| p11 `script-src 'nonce-k'; style-src 'nonce-k'`, оба с `nonce=k` | `/s4.js`, `/red4.css`; `ran=5` |

В логе на каждый пропуск — `⤷ preload пропущен (CSP): <url>`. Строки `⤷ preload css|js`
печатает `dispatch_preload_hints`: это событие для devtools, не запрос.

Побочные находки: `<link nonce>` под `style-src 'nonce-…'` не применяется окончательным
гейтом ([BUG-1486](BUG-1486-OPEN.md), на p11 `#a` остаётся чёрным); прогрев
`preload`/`modulepreload`/`prefetch` по-прежнему идёт в обход CSP ([BUG-1487](BUG-1487-OPEN.md)).
