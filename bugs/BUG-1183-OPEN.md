# BUG-1183 — CSP: `*-src-elem`/`script-src-attr` разбираются, но не участвуют в решении; `<script>` сообщает `script-src`

**Статус:** OPEN
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
