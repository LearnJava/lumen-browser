# BUG-1464 — `<link rel=stylesheet nonce>` блокируется под `style-src 'nonce-…'`

**Статус:** OPEN
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1185](BUG-1185-FIXED.md); свидетель — свой сервер).
**Область:** shell (`crates/shell/src/stylesheets.rs:141` — гейт `<link rel=stylesheet>` зовёт
`csp_enforce::style_src_blocked`, а тот спрашивает URL-only `fetch_directive_allows`; то же на
`:400`, `:535` и в событиях `page_pipeline.rs:1682`, `frames.rs:2561`).

## Симптом

`Content-Security-Policy: script-src 'nonce-k'; style-src 'nonce-k'`,
`<link nonce=k rel=stylesheet href=/red4.css><div id=a>A</div>`, лист `#a{color:rgb(255,0,0)}`.

| | Lumen dev-release | Chrome 153 |
|---|---|---|
| `getComputedStyle(a).color` | `rgb(0, 0, 0)` | `rgb(255, 0, 0)` |

Скрипт с тем же nonce (`<script nonce=k src=/s4.js>`) исполняется: гейт скриптов с BUG-1124 идёт
через `script_element_fetch_allows`. Для `<link>` nonce элемента не передаётся вовсе, и
nonce-only список читается как «ни один источник не подходит». Проверка с nonce уже есть —
`CspPolicy::style_element_fetch_allows` (BUG-1175, пока зовётся только из JS-гейта
`element_src_gate` и раннего прогрева BUG-1185).

## Репро

Сервер `target/bug1183/server.py` (worktree `p3-work`), страница `/p11.html`; проба
`target/bug1183/lumen_probe.py`.

## Что сделать

В окончательном гейте `<link rel=stylesheet>` читать `nonce` элемента и спрашивать
`style_element_fetch_allows`; события `securitypolicyviolation` — тем же предикатом.
Критерий: на `/p11.html` `a` красный, события нет; без nonce — чёрный и одно событие.
