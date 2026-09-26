# BUG-1185 — ранняя предзагрузка запрашивает `<script src>`/`<link>`, запрещённые CSP

**Статус:** OPEN
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1183](BUG-1183-FIXED.md); свидетель — свой сервер).
**Область:** shell (`crates/shell/src/page_load.rs:2581` `feed_preload_and_emit` — ранний
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
