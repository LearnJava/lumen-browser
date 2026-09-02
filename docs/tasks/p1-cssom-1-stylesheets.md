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

## Срез 2 (следующий) — реестр «узел → Stylesheet» на Document

Цель: `Document` получает поле вида
`stylesheet_nodes: Vec<(NodeId, Arc<Stylesheet>, DisabledFlag)>`
(порядок = порядок появления узлов в дереве), заполняемое в
`crates/shell/src/stylesheets.rs`/`page_pipeline.rs` рядом с существующим
`load_linked_stylesheets`/`build_page_cascade` — те продолжают строить единый
cascade-sheet, это отдельный (более дешёвый) параллельный парсинг per-node.
Здесь же обновляется `stylesheet_link_fingerprint`/`inline_style_fingerprint`
routing уже существует (BUG-443) — новый путь не должен его дублировать.

Из скоупа среза 2: `disabled`, `media`/`@import` conditional-активность на
уровне отдельного `<link>` (эти поля заводятся, но логика включения/отключения
— срез 4/CSSOM-3).

## Срез 3 — JS-биндинги, только для чтения

`document.styleSheets`, `<style>/<link>.sheet`, `CSSStyleSheet.cssRules`,
`CSSStyleRule.selectorText`/`style.cssText`, `CSSMediaRule.media.mediaText`
как объекты, построенные над данными среза 2 + сериализацией среза 1.

Ориентир по архитектуре (см. отчёт разведки 2026-09-03, доступен в истории
сессии): списочные DOM-объекты в этом движке (`NodeList`/`HTMLCollection`,
`document.images`) **не** имеют общего Rust-side "Vec-as-JS-object" хелпера —
вся логика индексации/`length`/`item()` строится в JS поверх нативов,
отвечающих "дай текущие id" и "опиши id N"
(`_lumen_make_nid_collection`, `crates/js/src/shim/web_api_shim_mid.js:3859`).
`CSSRuleList`/`StyleSheetList` следует той же схеме: натив
`_lumen_stylesheet_rule_count(sheetId)` / `_lumen_stylesheet_rule_get(sheetId,
idx)` (JSON), обёрнутый в `_lumen_make_nid_collection`-подобный Proxy (близкий
твин, не по id узлов, а по индексам правил).

Класс-объект с состоянием (id таблицы стилей, не весь снапшот) — по образцу
`Range` (`_lumen_make_range`, `crates/js/src/shim/web_api_shim_mid.js:6372`):
JS-объект хранит только адресуемый Rust-стороной идентификатор, каждый метод
дёргает свежий натив.

Документ передаётся в install-функцию по значению (`Arc<Mutex<Document>>` как
аргумент, не `thread_local!`) — см. `install_document_fonts`
(`crates/js/src/v8_runtime/install/dom_core.rs:91`) как образец; `thread_local`
допустим только если значение читается заново при каждом вызове натива через
свой accessor (образец — `NAMED_ACCESS_DOC`,
`crates/js/src/v8_runtime/named_access.rs:30`), не однократно на install.

## Срез 4 (= CSSOM-5/BUG-897, отдельная ROADMAP-строка) — запись

`insertRule`/`deleteRule`, `new CSSStyleSheet()`, `adoptedStyleSheets` как
настоящий аксессор. Требует: мутация `Stylesheet` идёт через
`Stylesheet::merge_from`/`mark_mutated` (гейт
`every_stylesheet_mutation_in_the_workspace_announces_itself` уже это
проверяет), и переинвалидацию `stylesheet_link_fingerprint` при вставке
правила в живую таблицу стилей — иначе кэш каскада не увидит изменения
(тот же класс бага, что BUG-443).
