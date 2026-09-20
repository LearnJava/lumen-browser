# Задача: fetch Priority Hints + 103 Early Hints

**Developer:** P1
**Ветка:** `p1-early-hints`
**Размер:** M
**Крейты:** `lumen-network`, `lumen-js`, `lumen-core`

## Goal
1. **Priority Hints** (HTML LS §17.2.3 / Fetch §2.2): `fetchpriority="high|low|auto"` на
   `<img>`/`<link>`/`<script>` + `{priority}` в `fetch()`-init → влияние на порядок выборки.
2. **103 Early Hints** (RFC 8297): обработать informational 1xx-ответ с `Link: rel=preload`,
   начать preconnect/preload подресурсов до финального ответа.

## Current state (сверено с кодом 2026-07-05)
### Priority — частично есть на уровне ядра
- `crates/core/src/event.rs:66-88` — enum `FetchPriority { High=0, Medium=1, Low=2 }`
  + `for_kind()` (эвристика по типу подресурса: CSS/шрифт=High, script=Medium, img=Low).
- `crates/core/src/event.rs:109-113` — `Event::SubresourceHintFound { url, kind, priority }`
  (preload-сканер уже проставляет приоритет по типу).
- **НЕТ**: чтения HTML-атрибута `fetchpriority` (grep по `fetchpriority` в `crates/` — 0);
  приоритет считается только эвристикой по типу, автор-override игнорируется.
- **НЕТ**: `{priority}` в `fetch()`-init на JS-стороне.
- RFC 9218 `Priority:` заголовок в исходящем запросе: упоминается только в fingerprint-
  профиле Firefox (`crates/network/src/http/headers.rs:229`), не привязан к FetchPriority.

### 103 Early Hints — НЕТ (главный гэп)
- `crates/network/src/lib.rs:418-463` — `read_head()` читает ПЕРВУЮ status-line и трактует
  её как ФИНАЛЬНУЮ (`parse_status` на `lib.rs:429`). **1xx informational не пропускаются.**
  103-ответ будет ошибочно принят за финальный статус, тело сломается.
- Grep `103`/`Early.?Hints` по `crates/**/*.rs` → совпадения только в бинарных/несвязанных
  местах (qpack, hpack, icc); реальной обработки Early Hints нет.

## Entry points
- `crates/network/src/lib.rs:418` — `read_head()` (сюда добавить цикл пропуска 1xx + сбор 103).
- `crates/network/src/lib.rs:429` — `parse_status` (различить 1xx vs финальный).
- `crates/core/src/event.rs:66` — `FetchPriority` (сюда добавить `Auto`/author-override).
- `crates/core/src/event.rs:109` — `SubresourceHintFound` (учесть explicit fetchpriority).
- HTML-атрибут `fetchpriority`: искать место парсинга `<link rel=preload>`/`<img>` в
  preload-сканере (`SubresourceHintFound`-эмиттер) и в `lumen-dom`.
- `crates/js/src/dom.rs` — `fetch()` init-объект (добавить чтение `priority`).

## Срезы (декомпозиция)
### Срез 1 — S — `read_head` пропускает 1xx (кроме 103)
В `read_head` (`lib.rs:418`): если status ∈ [100,199] и ≠103 → отбросить заголовки этого
блока и читать следующую status-line (цикл). Это чинит и потенциальный `100 Continue`.
Юнит-тест: mock-сервер шлёт `100 Continue\r\n\r\n` перед `200 OK`.

### Срез 2 — M — Парсинг 103 Early Hints — **сделано 2026-09-20 (P1)**
`read_head` (`crates/network/src/http1/response.rs`) собирает сырые значения `Link:` из
каждого `103`-блока в `early_hint_links: Vec<String>` (новый 4-й элемент `ResponseHead`,
новое приватное поле `Response`), затем продолжает цикл до финального статуса. Проброшено
до публичного `PageResponse::early_hint_links` (`crates/network/src/lib.rs`) — оба пути,
`fetch_page`/`fetch_page_streaming`. **Только HTTP/1.1** — H2 (`h2::conn::fetch_with_body`)
и H3 (`h3::h3_exchange`, поле `H3Response.informational` хранит только коды статусов) по
прежнему отбрасывают 1xx-заголовки целиком; `Response::early_hint_links` для них — пустой
`Vec` с комментарием-пометкой. Юнит-тест: `fetch_page_collects_early_hint_link_headers_from_103`
(`crates/network/src/lib.rs`), два последовательных `103` с разными `Link` перед `200 OK`.

### Срез 3 — S — Проброс Early Hints в preload-конвейер — **сделано 2026-09-20 (P1)**
`lumen_html_parser::preload_scanner::parse_link_header` (новая функция, реэкспорт
`lumen_html_parser::parse_link_header`) парсит одно значение HTTP-заголовка `Link`
(RFC 8288 §3: `<url>; rel=…; as=…`, запятая — разделитель элементов верхнего уровня,
не внутри `<…>`/кавычек) в тот же `Vec<PreloadHint>`, что и HTML-сканер, переиспользуя
его `rel`-словарь (`preload`/`preconnect`/`dns-prefetch`/`modulepreload`/`prefetch`/
`stylesheet`). `crates/shell/src/page_source.rs`: новая `emit_early_hints()` вызывается
из `load_bytes`/`load_bytes_streaming` сразу после `fetch_page`/`fetch_page_streaming`
возвращаются (`sink` теперь клонируется перед `.with_sink()`, чтобы остаться доступным)
— URL резолвится от `final_url` (BUG-757-совместимо), хинты уходят в
`dispatch_preload_hints` (`page_pipeline.rs:121`) тем же путём, что и author-хинты из
разметки. Не «до body» (клиент `lumen-network` синхронный из конца в конец), но строго
раньше, чем те же `<link>` увидел бы HTML preload-сканер — тело ещё не распарсено.
Дедуп — call-local `HashSet` (не общий с `preload_seen` HTML-скана): `SubresourceHintFound`
сегодня только строка в stderr-логе (реальный fetch делает JS-шим на DOM-элементе),
поэтому цена возможного дубликата — одна лишняя строка лога, не лишний сетевой запрос.

### Срез 4 — S — HTML-атрибут `fetchpriority` — **сделано 2026-09-21 (P1)**
`lumen_html_parser::preload_scanner::normalize_fetch_priority` (`high`/`low`
case-insensitive → сохраняется, `auto`/отсутствие/опечатка → `None`) читает
`fetchpriority` в `collect_link_hints`/`collect_script_hint`/`collect_img_hint`;
новое поле `fetch_priority: Option<String>` на `PreloadHint::Stylesheet`/
`Script`/`Image`/`Preload` (на `<link>` — один атрибут тега, общий для всех
hint-ов multi-token `rel`). `Link`-заголовок (срез 3, `parse_link_header`)
осознанно всегда даёт `None` — RFC 8288 не определяет такой параметр, только
HTML-атрибут несёт author-override. Отдельный `Option<explicit>`-слой вместо
`FetchPriority::Auto`: новый `FetchPriority::from_attr(Option<&str>) -> Option<Self>`
(`crates/core/src/event.rs`) возвращает override только для `"high"`/`"low"`,
`page_pipeline.rs`'s `dispatch_preload_hints` берёт
`from_attr(fp).unwrap_or_else(|| for_kind(kind))` и для сортировки, и для
самого `Event::SubresourceHintFound.priority` — override переживает и
`sort_by_key`, и emit. 6 новых тестов `lumen-html-parser` (`link_fetchpriority_*`,
`img_fetchpriority_high`, `script_fetchpriority_low`, `link_header_never_sets_fetchpriority`),
1 новый тест `lumen-shell` (`dispatch_preload_hints_fetchpriority_overrides_heuristic`:
`<img fetchpriority=high>` → High вместо дефолтного Low, `<script fetchpriority=low>`
→ Low вместо дефолтного Medium). `cargo clippy -p lumen-core -p lumen-html-parser
-p lumen-shell --all-targets -D warnings` зелёный.

### Срез 5 — XS — `fetch(url, {priority})` на JS-стороне — **сделано 2026-09-21 (P1)**
`init.priority` (`'high'|'low'|'auto'`) уже читался и нормализовался в
`_lumen_fetch` (`web_api_shim_mid_b2.js`) — только нигде не использовался
дальше (комментарий «Phase 2: network priority queue» относился к реальной
приоритетной очереди в `lumen-network`, которой до сих пор нет). Дошито:
явный `'high'`/`'low'` теперь уходит в запрос как RFC 9218 `Priority:`
заголовок (`u=1`/`u=5`, urgency 0..7, дефолт u=3) через уже существующий
`authorHeaders`-канал (BUG-749) — тот же путь, что и author-заголовки,
поэтому явный `Priority` из `init.headers`/`Request.headers` вытесняет
маппинг из `priority`, а не дублирует его. `'auto'` (дефолт, невалидное
значение, отсутствие опции) заголовок не шлёт вовсе — сервер/эвристика
UA решают сами. `Request.priority` (constructor/property) сознательно вне
скоупа — задача касалась только `fetch()`. 2 новых теста `lumen-js`
(`v8_whatwg_streams.rs`): `fetch_priority_maps_to_rfc9218_priority_header`,
`fetch_priority_author_header_overrides_init_priority`; уже существующие
`fetch_priority_high_and_low_accepted`/`fetch_priority_invalid_normalizes_to_auto`
(`v8_page_visibility_beacon.rs`) покрывали «не бросает». `cargo clippy -p
lumen-js --all-targets --features v8-backend -D warnings` зелёный.

### Срез 6 — XS — Доки — **сделано 2026-09-21 (P1)**
`CAPABILITIES.md` получил bullet в `## Networking & storage` (сразу после content-decoding);
`ROADMAP.md:244` (`P3-earlyhints`) переведён из `planned` в `done`; `subsystems/network.md`
и `subsystems/js.md` получили по одной строке про `Priority:`/`fetchpriority`/103.

## Tests
- `lumen-network`: skip 1xx (`100 Continue`), парсинг 103 + `Link`, финальный статус корректен.
- `lumen-network`: 103 с несколькими `Link` → несколько preload-хинтов.
- `lumen-js`: `fetch(u,{priority:'high'})` не бросает; атрибут `fetchpriority` читается.
- Регресс: обычный ответ без 1xx работает как раньше (`read_head` не изменил семантику).

## Definition of done
- [x] `read_head` пропускает informational 1xx, финальный статус читается верно.
- [x] 103 Early Hints парсятся, `Link: rel=preload/preconnect` эмитят подресурс-хинты.
- [x] `fetchpriority` HTML-атрибут переопределяет эвристику приоритета.
- [x] `fetch()` init читает `priority`.
- [x] Тесты зелёные; `CAPABILITIES.md`/`ROADMAP.md`/`subsystems/` обновлены.

Задача `P3-earlyhints` полностью `done` — H2/H3 1xx-гэп и сетевая приоритетная
очередь (RFC 9218 — сейчас advisory-only заголовок, никто не переупорядочивает
запросы) осознанно оставлены вне скоупа этого брифа, задокументированы как гэп
в `CAPABILITIES.md`.
