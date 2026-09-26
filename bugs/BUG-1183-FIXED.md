# BUG-1183 — CSP: `*-src-elem`/`script-src-attr` разбираются, но не участвуют в решении; `<script>` сообщает `script-src`

**Статус:** FIXED 2026-09-26 (P3)
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1181](BUG-1181-FIXED.md); найден чтением кода, живой
пробы нет).
**Область:** network (`crates/network/src/csp.rs:199` `effective_sources`, `:248`
`script_element_fetch_allows`, `:277` `style_element_fetch_allows`); shell
(`crates/shell/src/csp_enforce.rs:375`, `:386`, `:497`, `:592` — гейты; `:479`
`fire_script_src_violation`).

## Симптом

1. **Гранулярные директивы игнорируются при решении.** `parse_csp_header` кладёт
   `script-src-elem`, `style-src-elem`, `script-src-attr` в `directives`
   (`csp.rs:591`–`:595`), но ни один гейт их не читает: все зовут
   `effective_sources(ScriptSrc | StyleSrc)`, а тот падает только на `default-src`.
   Ожидаемо, что `Content-Security-Policy: style-src-elem 'none'` блокирует
   `<link rel=stylesheet>` и `<style>`, а `script-src-elem 'none'` — `<script>`; сейчас оба
   проходят. Обратный случай: `style-src 'none'; style-src-elem 'self'` должен пропустить
   лист со своего origin, сейчас блокирует. Исключение — `style-src-attr`: для атрибута
   `style` fallback `style-src-attr → style-src` уже есть (`csp_enforce.rs:414`).
2. **`violatedDirective` у скриптов.** `fire_script_src_violation` (`csp_enforce.rs:479`)
   передаёт литерал `script-src` и для inline `<script>`, и для парсерного `<script src>`.
   CSP3 §6.8.2/§6.8.3 дают эффективную директиву `script-src-elem` (у вставленного скриптом
   `<script src>` это уже так — [BUG-1175](BUG-1175-FIXED.md)). Для листов то же чинил
   [BUG-1181](BUG-1181-FIXED.md).

## Что сделать

- В `CspPolicy` добавить fallback-список CSP3 §6.8.4 «Get the effective directive … fallback
  list»: `script-src-elem → script-src → default-src`, `script-src-attr → script-src →
  default-src`, `style-src-elem → style-src → default-src`, `style-src-attr → style-src →
  default-src`. Перевести на него `script_element_fetch_allows`,
  `style_element_fetch_allows`, inline-гейты `csp_enforce.rs:375`/`:386` и URL-гейты
  `:497`/`:592` (для `<script src>` и `<link>`/`@import` запрос имеет destination
  `script`/`style` → `*-elem`).
- `fire_script_src_violation` — `script-src-elem`.
- Перед правкой снять живую пробу против Chrome: `style-src-elem 'none'` + `<link>`,
  `script-src-elem 'none'` + inline `<script>`, `style-src 'none'; style-src-elem 'self'`.

Критерий: три страницы выше совпадают с Chrome по блокировке и `violatedDirective`.

## Решение (2026-09-26, P3)

`CspPolicy::effective_sources` (`crates/network/src/csp.rs`) идёт по fallback-списку CSP3
§6.8.4: `script-src-elem`/`script-src-attr` → `script-src` → `default-src`,
`style-src-elem`/`style-src-attr` → `style-src` → `default-src`. Остальные директивы, как и
раньше, падают сразу на `default-src`; `frame-src`/`worker-src` сохраняют свою цепочку через
`child-src`.

Гейты спрашивают элементные директивы: `script_element_fetch_allows` и
`style_element_fetch_allows` — `ScriptSrcElem`/`StyleSrcElem`; inline `<script>` (`scripts.rs`,
классический и модульный) — `ScriptSrcElem`; inline `<style>` (`doc_extract.rs`),
`style_src_blocked` (`<link>`, `@import`) и поиск нарушенных политик листа (`page_pipeline.rs`,
`frames.rs`) — `StyleSrcElem`. Гейт атрибута `style` берёт `effective_sources(StyleSrcAttr)`
вместо своей копии цепочки. `fire_script_src_violation` сообщает `script-src-elem`.

Тесты: `csp::tests::granular_directives_follow_csp3_fallback_list`,
`csp::tests::element_checks_read_elem_directives` (lumen-network);
`csp_enforce::tests::style_src_elem_does_not_reach_attribute`,
`tests::page_pipeline::style_src_elem_decides_for_style_elements`,
`tests::page_pipeline::inline_script_is_gated_and_reported_as_script_src_elem` (lumen-shell).

### Живая проба

Сервер `target/bug1183/server.py` (worktree `p3-work`, CSP в заголовке ответа), Chrome 153
headless и Lumen dev-release `--maximized`, `LUMEN_NO_ADBLOCK=1`, чтение через MCP `eval`
через 4 с после навигации. `ev` — `violatedDirective|effectiveDirective|blockedURI`.

| Страница, политика | Chrome 153 | Lumen после правки |
|---|---|---|
| P1 `style-src-elem 'none'`, `<link>` + `<style>` | `#a` чёрный; `style-src-elem` для листа и inline | то же |
| P2 `script-src-elem 'nonce-x'`, inline + `<script src>` без nonce | `ran=0`; `script-src-elem` для inline и `s.js` | то же |
| P3 `style-src 'none'; style-src-elem 'self'`, свой `<link>` | `#a` красный, событий нет | то же |
| P4 `script-src 'nonce-x'`, inline + `<script src>` без nonce | `ran=0`; `script-src-elem` для обоих | то же |
| P5 `script-src 'none'; script-src-elem 'nonce-x' 'self'`, свой `<script src>` | `ran=2`, событий нет | то же |
| P6 `style-src-elem 'none'`, атрибут `style` | атрибут применён | то же |

Побочные находки той же пробы, не связанные с правкой (воспроизводятся и с обычными
`style-src 'none'`/`script-src 'none'`): `getComputedStyle` видит заблокированный `<style>`
([BUG-1184](BUG-1184-OPEN.md)); предзагрузка при потоковом разборе запрашивает
заблокированные `<script src>`/`<link>` ([BUG-1185](BUG-1185-OPEN.md)).
