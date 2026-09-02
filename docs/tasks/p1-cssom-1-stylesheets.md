# CSSOM-1 — объектная модель таблиц стилей

Бриф для `ROADMAP.md`-задачи `CSSOM-1` (владелец — P1, дорожка CSSOM, см. строку
756/757). Закрывает ядро BUG-471: `document.styleSheets`, `<style>`/`<link>.sheet`,
классы `CSSStyleSheet`/`CSSRule`/`CSSRuleList`/`CSSStyleRule`/`CSSMediaRule` как
глобалы, `insertRule`/`deleteRule`, `new CSSStyleSheet()`.

Размер задачи в ROADMAP.md — `L`. Срез 1 (парсерный фундамент) сделан и влит
2026-09-03 (`p1-cssom1`, коммит `4d636bdab`). Этот файл фиксирует, почему
оставшееся — не «дописать биндинги», а отдельная архитектурная работа, и как
её резать на срезы.

---

## Срез 1 (сделан, 2026-09-03) — сериализация в `lumen-css-parser`

- `Rule::selector_text()` / `Rule::style_css_text()`, `Declaration::to_css_text()`
  — дают `CSSStyleRule.selectorText` / `.style.cssText`.
- `MediaQuery::raw` — сырой текст между `@media`/`@import`-URL и `{`/`;`, источник
  `CSSMediaRule.media.mediaText`.
- `Stylesheet::top_level_order` + `Stylesheet::cssom_rules()` — восстанавливают
  исходный порядок style/`@media` правил (`rules`/`media_rules` — раздельные
  `Vec` без общего индекса).
- Попутно два реальных дефекта сериализации: `attr_to_css_str` терял
  закрывающую `]`, `PseudoClass::Has` печатал заглушку `":has(…)"` вместо
  содержимого (`relative_sels_to_css_str` добавлен).
- Тесты: `parser/tests/selectors.rs`, `parser/tests/revision.rs`
  (`rule_selector_and_style_text_serialize_for_cssom`,
  `cssom_rules_preserves_source_order_across_style_and_media`,
  `to_css_str_attribute_closes_bracket`, `to_css_str_has_...`).
- `top_level_order` добавлен в список полей мутационного гейта
  `every_stylesheet_mutation_in_the_workspace_announces_itself`.

Ничего из этого ещё не подключено к JS — чистое новое API-поверхностное
расширение `lumen-css-parser`, инертное до среза 3.

---

## Архитектурный пробел, найденный при скоупинге среза 2

Текущая модель данных даёт ровно **один смерженный `Stylesheet` на страницу**,
не по одному на `<style>`/`<link>`:

- `crates/shell/src/stylesheets.rs::load_linked_stylesheets` стягивает текст
  всех `<link>` (и разворачивает их `@import`) в одну строку; исходные
  `(NodeId, href)` пары используются только для `load`/`error`-событий, а не
  сохраняются.
- Итоговый текст (инлайновые `<style>` + слинкованный CSS) парсится **один
  раз** в `ParsedPage.stylesheet` (`crates/shell/src/page_pipeline.rs:239`) →
  `LayoutSource.stylesheet: Arc<Stylesheet>` (`:293`) → `PageCascade.sheet`
  (`:326`, строится `build_page_cascade`, `:341`).
- `Document` (`crates/engine/dom/src/lib.rs:330`) не хранит ссылок на
  стилевые данные вообще, кроме `fonts: FontFaceSet` (`@font-face`, `:356`).

Это значит: `document.styleSheets[i]` (по определению CSSOM — по одному входу
на каждый действующий `<style>`/`<link rel=stylesheet>`) сегодня физически
нечем ответить. Нужно либо (а) парсить каждый `<style>`/`<link>` отдельно
**в дополнение** к уже существующему смерженному cascade-sheet (безопаснее —
не трогает горячий путь стилизации), либо (б) выводить срез смерженного sheet
обратно по узлам через позиционную книгу учёта. **(а) выбран** — cascade
остаётся на едином `Stylesheet`, `styleSheets` получает свою параллельную,
более дешёвую структуру данных, которая не участвует в layout/paint.

---

## Срез 2 (сделан, 2026-09-03) — реестр «узел → Stylesheet»

Цель была: `Document` получает поле вида
`stylesheet_nodes: Vec<(NodeId, Arc<Stylesheet>, DisabledFlag)>`. **Место
хранения пересмотрено при реализации**: не `Document` (`lumen-dom`) — это
потребовало бы нового `lumen-dom → lumen-css-parser` зависимого ребра, а
CLAUDE.md/REVIEW.md держат `dom`/`css-parser` сиблингами одного яруса
(`lumen-core → dom/font/parsers → layout → paint → shell`), и прецедент уже
есть: `Document::fonts` — свой DOM-нативный `FontFace`, не
`lumen_css_parser::FontFaceRule`. Вместо этого реестр — параллельное поле
`LayoutSource`/`ParsedPage`/`PageCascade` в `crates/shell`, ровно там же, где
уже живёт единый `stylesheet: Arc<Stylesheet>`; `crates/js` и так зависит от
обоих (`lumen-dom` и `lumen-css-parser`), так что срезу 3 это ничего не
стоит — он читает `LayoutSource::stylesheet_nodes` тем же путём, каким
`computed_styles`/`custom_properties` уже идут из shell в `V8JsRuntime`
(`update_computed_styles`-подобный метод, срез 3).

Сделано:
- `StylesheetNodeEntry { node, sheet: Arc<Stylesheet>, disabled: bool }` +
  `build_stylesheet_node_registry()` (`crates/shell/src/stylesheets.rs`):
  один комбинированный обход дерева (не два раздельных, как
  `walk_style_blocks`/`collect_link_hrefs`) — иначе `<style>` и `<link>`
  вышли бы сгруппированными по тегу, а не в истинном порядке документа.
  `<link>`-тело читается через тот же `fetch_stylesheet_text`/
  `PREFETCH_CACHE`, что уже прогрет `load_linked_stylesheets` — без второго
  сетевого запроса, только лишний парс CSS на элемент.
- Вызывается внутри `build_page_cascade`, поэтому пересобирается вместе с
  каскадом (до скриптов и повторно после — если скрипты тронули
  `<style>`/`<link>`, тот же гейт BUG-443/`stylesheet_link_fingerprint`/
  `inline_style_fingerprint`, который уже есть, ничего дублировать не
  пришлось).
- Прокинуто через `PageCascade` → `ParsedPage` → `LayoutSource`
  (`Arc<Vec<StylesheetNodeEntry>>`, тот же cheap-clone паттерн, что у
  `stylesheet`). Пути восстановления (`bfcache.rs` × 2, `docking.rs`,
  `hibernation.rs`) получают пустой реестр — они и `dynamic_css: None` по
  той же причине: исходные `<style>`/`<link>` там не сохраняются.
- Медиа-гейт (`link_media_matches`) сюда не подключён: принадлежность листа
  `document.styleSheets` не зависит от текущего совпадения `media` (CSSOM);
  это осталось только в `collect_link_hrefs` для каскада.
- Тесты: `crates/shell/src/tests/page_resources.rs`
  (`stylesheet_node_registry_preserves_document_order_across_tags`,
  `stylesheet_node_registry_drops_unfetchable_link`).

Из скоупа среза 2 (как и было): `disabled` всегда `false`, `media`/`@import`
conditional-активность отдельного `<link>` — логика включения/отключения
остаётся срезу 4/CSSOM-3. Поля реестра сейчас нигде не читаются
(`#[allow(dead_code)]`) — потребитель появится в срезе 3.

## Срез 3 (сделано, 2026-09-03) — JS-биндинги, только для чтения

`document.styleSheets`, `<style>/<link>.sheet`, `CSSStyleSheet.cssRules`,
`CSSStyleRule.selectorText`/`style.cssText`, `CSSMediaRule.media.mediaText`
(+ вложенные правила `@media` через `CSSMediaRule.cssRules`) — объекты,
построенные над реестром среза 2 + сериализацией среза 1.

Реализация:
- `V8JsRuntime::stylesheet_nodes: Arc<Mutex<Vec<lumen_css_parser::StylesheetNodeEntry>>>`
  + `update_stylesheet_nodes()` — тот же паттерн, что `computed_styles`/
  `update_computed_styles`. Наполняется в `run_scripts_with_dom`
  (`crates/shell/src/scripts.rs`, сразу после `install_dom`, до первого
  скрипта — BUG-443 порядок) и повторно при перестройке каскада после
  скриптов (`page_pipeline.rs`). Фреймы/bfcache/docking/hibernation получают
  пустой реестр — тот же fallback, что срез 2 уже сделал для самого
  `stylesheet_nodes` в `LayoutSource`.
- Канонический тип реестра — `lumen_css_parser::StylesheetNodeEntry` (не
  shell-локальный, как планировалось в срезе 2): `crates/js` не может
  зависеть от `crates/shell` (наслоение), а `lumen-css-parser` — общий
  сиблинг обоих. `crates/shell/src/stylesheets.rs::StylesheetNodeEntry`
  теперь просто `pub(crate) use lumen_css_parser::StylesheetNodeEntry;`.
- Rust-натив `crates/js/src/v8_runtime/install/stylesheets.rs`: 6 нативов
  (`_lumen_stylesheet_owner_nids`/`_disabled`/`_rule_count`/`_rule_json`/
  `_media_child_count`/`_media_child_json`), правило и media-правило
  сериализуются в JSON через `serde_json::json!` (не строкой вручную).
- JS: **не** переиспользует `_lumen_make_nid_collection` (тот заточен под
  id узлов и зовёт `_lumen_make_element`) — общий Proxy-хелпер для
  индексных списков объектов сведён в новый `_lumen_make_indexed_list`
  (`web_api_shim_mid.js`, рядом с `_lumen_make_range`), которым пользуются
  и `CSSRuleList`, и `StyleSheetList`. Каждый JS-объект (`CSSStyleSheet`/
  `CSSStyleRule`/`CSSMediaRule`) адресует Rust-состояние по индексу и
  перечитывает натив на каждый доступ — образец `_lumen_make_range`
  подтвердился. Идентичность объектов НЕ сохраняется между чтениями
  (`sheet.cssRules[0] !== sheet.cssRules[0]`) — задокументированная
  упрощение, кэш обёрток по (sheetIdx, ruleIdx) остался вне среза 3.
- `<style>`/`<link>.sheet` — геттер на прототипе
  (`web_api_shim_tail_b.js`, рядом с `_lumen_install_reflection` для этих
  тегов), НЕ на общей `_LUMEN_WRAPPER_MEMBERS` (BUG-920-класс дефекта).
- Тесты: `crates/js/src/dom/tests/v8_cssom_stylesheets.rs` (7 тестов,
  V8JsRuntime напрямую, без реального `<link>`-фетча).

Вне скоупа (остаётся срезу 4 / CSSOM-2): `insertRule`/`deleteRule`,
`new CSSStyleSheet()`, мутируемый `CSSStyleDeclaration` (`style.cssText`
только для чтения), `stylesheet.disabled` запись, фреймы/сериализация
`@import`/`@font-face`/`@supports` как `CSSRule`.

## Срез 4 (= CSSOM-5/BUG-897, отдельная ROADMAP-строка) — запись

`insertRule`/`deleteRule`, `new CSSStyleSheet()`, `adoptedStyleSheets` как
настоящий аксессор. Требует: мутация `Stylesheet` идёт через
`Stylesheet::merge_from`/`mark_mutated` (гейт
`every_stylesheet_mutation_in_the_workspace_announces_itself` уже это
проверяет), и переинвалидацию `stylesheet_link_fingerprint` при вставке
правила в живую таблицу стилей — иначе кэш каскада не увидит изменения
(тот же класс бага, что BUG-443).
