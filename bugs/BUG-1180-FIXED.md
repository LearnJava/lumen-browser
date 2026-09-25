# BUG-1180 — CSP: вставленный скриптом `<link rel=stylesheet>` применяется в обход `style-src`

**Статус:** FIXED 2026-09-26 (P3)
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1175](BUG-1175-FIXED.md); видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`, сравнение с Chrome 153 тем же способом).
**Область:** shell (`crates/shell/src/stylesheets.rs:526` — `build_stylesheet_node_registry` →
`fetch_stylesheet_text`).

## Симптом

Страница с `Content-Security-Policy: style-src 'none'` вставляет скриптом
`<link rel=stylesheet href=/a.css>`. Элемент получает `error`, как в Chrome: JS-путь элемента
проверяет `style-src` с BUG-1175. Но сам лист всё равно скачивается и применяется: сервер видит
`GET /a.css` от Lumen, `#out` красный. Chrome лист не запрашивает, цвет остаётся чёрным.

Реестр листов `build_stylesheet_node_registry` качает `href` каждого `<link>` через
`fetch_stylesheet_text`. `csp_gate` он передаёт только ради `upgrade-insecure-requests`, а
`style_src_blocked` не вызывает. `load_linked_stylesheets` (`:141`) и `inline_css_imports`
(`:400`) эту проверку делают.

## Репро

Сервер `.tmp/compat/b1175/server.py` (worktree `p3-work`), страница `/link_style_self.html`:
CSP `style-src 'none'`, скрипт вставляет `<link rel=stylesheet href=/a.css>` (`#out{color:red}`)
и пишет `load`/`error` в `window.EV`. Проба через 3 с после `document_ready`:

| | Lumen | Chrome 153 |
|---|---|---|
| `EV` | `error` | `error` |
| `getComputedStyle(#out).color` | `rgb(255, 0, 0)` | `rgb(0, 0, 0)` |
| `GET /a.css` на сервере | есть | нет |

## Что сделать

В `build_stylesheet_node_registry` для `StylesheetOwner::Link` проверять
`csp_enforce::style_src_blocked` по апгрейженному URL, как это делает `load_linked_stylesheets`.
Заблокированный лист в реестр не попадает, запроса нет. Проверить, что `securitypolicyviolation`
для такого `<link>` приходит ровно один раз: сейчас его шлёт только `page_pipeline.rs:1655`
(срез 7) по `blocked_by_style_src`. Правка двигает пиксели, поэтому нужен полный прогон
`graphic_tests`.

Критерий: `/link_style_self.html` совпадает с Chrome по всем трём строкам таблицы.

## Исправление (2026-09-26, P3)

`build_stylesheet_node_registry` для `StylesheetOwner::Link` проверяет
`csp_enforce::style_src_blocked` по апгрейженному URL до `fetch_stylesheet_text`, тем же порядком,
что `load_linked_stylesheets`. Заблокированный лист в реестр не попадает, запроса нет. Нарушение
реестр не сообщает: событие уже шлёт путь каскада (`page_pipeline.rs`, срез 7).

Тест `stylesheet_node_registry_skips_style_src_blocked_link` (`crates/shell/src/tests/page_resources.rs`):
лист лежит на диске, `style-src 'none'` в `<meta>`, реестр пуст. Без правки тест падает.

Проба (видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, Chrome 153), 3 с после `document_ready`:

| | Lumen | Chrome 153 |
|---|---|---|
| `/link_style_self.html`: `EV` | `error` | `error` |
| `getComputedStyle(#out).color` | `rgb(0, 0, 0)` | `rgb(0, 0, 0)` |
| `GET /a.css` на сервере | нет | нет |
| `/link_style_spv.html`: события `securitypolicyviolation` | одно | одно |

Критерий выполнен. Попутно на `/link_style_spv.html` видны два расхождения, не входящие в этот баг:
`violatedDirective` у Lumen `style-src`, у Chrome `style-src-elem` ([BUG-1181](BUG-1181-FIXED.md));
`document.styleSheets.length` у Lumen `0`, у Chrome `1`. Второе — не дефект: Chrome держит пустой
лист у любого `<link>` с провалом загрузки (404: `cssRules.length == 0`; CSP: `cssRules` бросает
`SecurityError`), а HTML «process the linked resource» создаёт лист только при `success`. Lumen
следует спецификации (`element.sheet === null`, как и в `stylesheet_node_registry_drops_unfetchable_link`).
