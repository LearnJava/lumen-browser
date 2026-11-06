# BUG-1694 — Документ на ≈850 КБ с ≈17 000 `<span>` (`display:none`) загружается ≈21 с и обрывает навигацию родителя по таймауту

**Статус:** FIXED 2026-11-06 (P3)
**Заведён:** 2026-10-23 (P2, WPT-ENC-SCOUT)
**Область:** js/v8 (`v8_runtime/named_access.rs`, `v8_runtime/install/dom_core.rs`, `shim/web_api_shim_mid.js`)

## Симптом

`encoding/legacy-mb-*/*-decode-*.html` встраивают iframe `*_chars-*.html` — один документ ≈850 КБ с ≈17 000 `<span>` с `display:none`. В одиночном запуске «Распарсено» наступало через ≈21 с; под 10 параллельными браузерами навигация родителя обрывалась по `DocumentReady` (30 с): 321 id ERROR в WPT-RUN-16.

## Диагностика

Профилирование `install_dom` на сгенерированном документе (17 000 `<span style="display:none">`, ≈600 КБ): `V8JsRuntime::install_dom` занимал 4–8,5 с против 184 мс на tiny-документе; разбивка показала, что дороже всего обходились не парсер и layout, а исполнение JS-шима и batch module-установок — причём стоимость росла линейно по числу элементов, а не по байтам.

Инструментация (временная, по нативным вызовам и JS-меткам) выявила два независимых O(элементы) источника:

1. **Window named-property interceptor** (`v8_runtime/named_access.rs`, BUG-384): на каждый промах глобального имени V8 вызывает перехватчик, а `named_access_lookup` делал **полный обход дерева** `find_first_matching` — `get_attr("id")`/`get_attr("name")` на каждом узле. Шим при установке копирует ~270 собственных свойств `window` на `globalThis` (`web_api_shim_tail_b.js`, секция «window IS the real global object») — каждая запись `globalThis[k] = v` по отсутствующему имени запускала обход всех ~17 000 узлов: ≈255 × ≈5 мс ≈ 1,2 с только в этом цикле; тот же штраф платили module-шимы и любые page-скрипты с глобальными промахами.
2. **`getElementsByTagName(NS)`** (`shim/web_api_shim_mid.js`): JS-реализация делала `_lumen_query_selector_all(_scoped)(…, '*')`, затем `_lumen_collect_matching` с двумя переходами JS↔Rust (`_lumen_get_local_name` + `_lumen_get_namespace_uri`) на каждый элемент — на документе в 17 000 элементов каждый parser-end scan (`_lumen_link_hints_scan`, `_lumen_script_empty_src_scan`, `_lumen_style_blocks_scan`, `_lumen_embed_object_scan`, `_lumen_details_open_scan`) стоил секунды.

## Исправление

- `named_access.rs`: потоковый слот `NAMED_ACCESS_DOC` заменён на `NamedAccessState { doc, touched, index_epoch, index }`. `set_named_access_document` теперь принимает и `Arc<Mutex<DomTouched>>`; `named_access_lookup` вместо обхода на имя держит **индекс `id`/`name` → первый NodeId в порядке дерева** (`build_named_access_index`, один pre-order проход, та же семантика: `id` на любом элементе, `name` только на `img`/`form`/`iframe`/`embed`/`object`, без спуска в shadow root). Индекс перестраивается при смене `DomTouched::epoch` — любой отслеживаемый примитив мутации (`setAttribute`, child-list, unattributed) инвалидирует его, поэтому ответ не может устареть. `v8_runtime.rs::install_dom` передаёт `dom_touched` в `set_named_access_document`.
- `getElementsByTagName`/`getElementsByTagNameNS` на `Document` и `Element` переведены на нативные `_lumen_get_elements_by_tag(_scoped)`/`_lumen_get_elements_by_tag_ns(_scoped)` (`install/dom_core.rs`): один нативный проход `query_all(_within)(…, "*")` + фильтр `tag_name_matches`/`tag_ns_matches` — те же правила, что кодировали JS-предикаты `_lumen_tag_name_predicate`/`_lumen_tag_ns_predicate` (BUG-416): HTML-неймспейс матчит имя без учёта регистра обеих сторон, чужой — точным qualified name, `'*'` — все элементы; scoped-вариант сохраняет stale-id guard BUG-919. Нормализация `namespace` (`null`/`undefined`/`''` → отсутствие неймспейса) вынесена в `_lumen_ns_arg`.

## Проверка

- Замер `--dump-layout` на документе 17 000 `<span style="display:none">`: `install_dom` 4,19 с → **0,199 с** (WEB_API_SHIM eval 1,66 с → 78 мс, module batch 2,49 с → 113 мс); полный прогон страницы ≈1,1 с. До исправления второй источник давал ≈8,5 с.
- `cargo test -p lumen-js --features v8-backend named_access` — 2/2 ok, включая `named_access_follows_live_dom_mutations` (инвалидация по epoch).
- `cargo test -p lumen-js --features v8-backend --lib tag_name` — 8/8 ok (case-insensitive HTML / case-sensitive foreign / `'*'` / NS-вариант / scoped).
