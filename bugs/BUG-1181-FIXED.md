# BUG-1181 — CSP: `securitypolicyviolation` для листов и `<style>` сообщает `style-src`, а не `style-src-elem`

**Статус:** FIXED 2026-09-26 (P3)
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1180](BUG-1180-FIXED.md); видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`, сравнение с Chrome 153 тем же способом).
**Область:** shell (`crates/shell/src/page_pipeline.rs:1678`, `:1681`, `:1696`;
`crates/shell/src/frames.rs:2397`, `:2400`, `:2405`; `crates/shell/src/relayout.rs:136`).

## Симптом

Страница с `Content-Security-Policy: style-src 'none'` вставляет скриптом
`<link rel=stylesheet href=/a.css>` и слушает `securitypolicyviolation`. Событие приходит ровно одно
в обоих браузерах, но `violatedDirective` разный:

| | Lumen | Chrome 153 |
|---|---|---|
| `e.violatedDirective + ' ' + e.blockedURI` | `style-src http://127.0.0.1:8735/a.css` | `style-src-elem http://127.0.0.1:8735/a.css` |

CSP3 §6.8.2 «Get the effective directive for request»: у запроса с destination `style` эффективная
директива `style-src-elem`, а `violatedDirective` равен `effectiveDirective`. Это верно и когда в
политике есть только `style-src` (к ней ведёт fallback-список директивы). Для inline `<style>`
действует то же правило: §6.8.3 даёт `style-src-elem` для типа `style`.

Все вызовы `fire_csp_violation` для листов и `<style>` передают литерал `"style-src"`: каскад
документа (`page_pipeline.rs`), iframe (`frames.rs`), relayout (`relayout.rs`). `style-src-attr`
для атрибута `style` уже передаётся верно.

## Репро

Сервер `.tmp/compat/b1175/server.py` (worktree `p3-work`), страница `/link_style_spv.html`.
Проба `window.SPV` через 3 с после `document_ready`.

## Что сделать

Передавать `style-src-elem` для `<link rel=stylesheet>`, `@import` и inline `<style>`. Проверить,
что `effectiveDirective` в событии тот же. Родственный случай — парсерный `<script src>` сообщает
`script-src` вместо `script-src-elem` ([BUG-1175](BUG-1175-FIXED.md), «Не сделано»).

Критерий: `/link_style_spv.html` даёт `style-src-elem`, как Chrome.

## Решение (2026-09-26, P3)

Все шесть вызовов `fire_csp_violation` для листа, `@import` и inline `<style>` передают
`style-src-elem`: каскад документа (`page_pipeline.rs`), iframe (`frames.rs`), relayout
(`relayout.rs`). `@import` отдельного вызова не имеет — его заблокированные URL уже
подмешаны в `blocked_by_style_src`. `effectiveDirective` берётся из того же аргумента
(`_lumen_dispatch_csp_violation`, `crates/js/src/csp.rs`), так что оба поля совпадают.

Тест: `tests::page_pipeline::style_src_violations_report_style_src_elem` — `<link>` и
`<style>` под `style-src 'none'` дают `style-src-elem/style-src-elem` (до правки
`style-src/style-src`).

Живая проба `/link_style_spv.html` (dev-release, `--maximized`, `LUMEN_NO_ADBLOCK=1`):
`SPV = ["style-src-elem http://127.0.0.1:8735/a.css"]`, `EV = ["error"]`, запроса `/a.css`
нет — как у Chrome 153 в таблице выше.

Не сделано: парсерный `<script src>` по-прежнему сообщает `script-src`, а директивы
`style-src-elem`/`script-src-elem`/`script-src-attr` в самой политике не участвуют в
принятии решения — это [BUG-1183](BUG-1183-OPEN.md).
