# CSSOM-10 — CSSOM для at-правил и вложенных правил (заметка передачи, P1)

Ветка `p1-cssom-10`, слот `p1-work`. Строка задачи — `ROADMAP.md` (CSSOM-10), дефект — `bugs/BUG-1455-OPEN.md`.
Критерий готовности: `tests/wpt/css/css-cascade/layer-cssom-order-reverse.html`, `css-nesting/cssom.html`,
`css-cascade/scope-cssom.html` зелёные (+ таблица проб из BUG-1455: `insertRule` для всех at-правил, `cssRules` их видит).

## Состояние

Кода пока нет — только разведка и проект ниже. Следующая сессия начинает с шага 1.

## Как устроено сейчас (разведка)

- `crates/engine/css-parser/src/parser.rs`: `Stylesheet` — типизированные коллекции (`rules`, `media_rules`, `layers`,
  `scope_rules`, …) + `top_level_order: Vec<TopLevelRuleKind>` (только `Style`/`Media`/`Mixin(idx)`). `cssom_rules()`
  отдаёт `CssomRuleRef::{Style,Media,Mixin}`; `insert_rule`/`delete_rule`/`set_rule_style_text`/… правят коллекции.
  Вложенные правила на разборе разворачиваются в плоские `Rule` (`nesting_expand.rs`) — вложенность теряется.
  `Stylesheet.source: Option<Arc<str>>` — исходный текст (`parse()` его ставит), нужен для `locate_embedded_source`.
- `crates/js/src/v8_runtime/install/stylesheets.rs` — нативы `_lumen_stylesheet_*` (владельческие листы, реестр
  `V8JsRuntime::stylesheet_nodes: Vec<StylesheetNodeEntry{node, sheet: Arc<Stylesheet>, disabled}>`);
  `install/constructed_stylesheets.rs` — `_lumen_constructed_*` (реестр `ConstructedStylesheet{sheet, disabled}`).
- JS: `crates/js/src/shim/web_api_shim_mid.js` ~12090–12650: `_lumen_make_css_style_sheet`, `_lumen_make_css_rule`,
  `_lumen_build_css_style_rule`, `CSSMediaRule`, `CSSMixinRule`, конструируемые листы-двойники; идентичности обёрток нет
  (`cssRules[0] !== cssRules[0]`), а `css-nesting/cssom.html` требует `cssRules[0].cssRules[1].parentRule === cssRules[0]`.
  `.style` правила — `_lumen_make_rule_style(loc)` (~3861) + ветвь `__loc__` в `_lumen_style_get_parsed/set_parsed` (~3561–3600).
  Геттер `.sheet` — `web_api_shim_tail_b.js:2261`.
- Синхронизация с каскадом: журнал `CssomDeltaLog` (`sheet_sync.rs::SheetSync::record`), `patched_cascade()` в
  `sheet_sync.rs` воспроизводит `CssomOp` на клоне каскадного листа (`Stylesheet::replay_cssom_ops`), узел ищется в
  тексте каскада через `locate_embedded_source(entry.sheet.source())`. `entry_is_current` сравнивает `sheet.source()`
  с текстом `<style>` — поэтому `source()` изменённого листа ОБЯЗАН остаться исходным текстом DOM.
- Тесты, которые нельзя сломать: `crates/js/src/dom/tests/v8_cssom_stylesheets.rs`, `v8_bug493_sheet_registry.rs`,
  `v8_cssom5_constructed_stylesheets.rs`, `v8_bug1130_shadow_root_members.rs`, `crates/js/tests/cases/bug518_mixin_cssom.rs`,
  `crates/engine/css-parser/src/parser/tests/revision.rs`; gate «every_stylesheet_mutation_in_the_workspace_announces_itself»
  (прямые push/extend в коллекции правил запрещены вне parser.rs).

## Проект (принят)

**Отдельное дерево CSSOM**, не расширение типизированных коллекций (вложенность в них не хранится).

1. `crates/engine/css-parser/src/cssom_tree.rs` (+ подмодули, файлы < 800 строк, `#![deny(missing_docs)]`-совместимо):
   - `CssomTree { rules: Vec<CssomNode> }`, `CssomNode { id: u32 (глобальный AtomicU32 — id уникальны на процесс, чтобы
     устаревшие JS-обёртки не путались), kind: RuleKind, prelude: String, decls: Vec<Declaration>, children: Vec<CssomNode>,
     raw: Option<String> }`. `raw` — точный исходный срез нетронутого узла.
   - Универсальный разборщик уровня правил: элемент = до `;` или до `{…}` с учётом строк/скобок/комментариев; у `@` —
     at-правило (имя → `RuleKind`; неизвестные отбрасываются), иначе вложенное стилевое правило. В теле стилевого правила
     и группы: декларации до первого вложенного правила → `decls`; после — узлы `NestedDeclarations`. Декларации разбирать
     через `parse_inline_style`. `@apply` внутри стилевого тела — узел-оператор (формат — как у `Rule::css_text`).
     `custom-property: {…};` — декларация, не правило.
   - Сериализация `css_text()` в формате Chrome: стилевое без детей `sel { a: b; }` (пустое — `sel { }`); с детьми —
     `sel {\n  a: b;\n  <child.cssText с отступом только первой строки>\n}`; группа `@media x {\n  child\n}` (пустая —
     `@scope {\n}`); лист-оператор `@layer a, b;`, `@import url("x") layer(a);`, `@namespace x url("a");`;
     лист с декларациями `@font-face { … }`, `@property --x { … }`, `@page :first { … }`; `@keyframes k { from {…} … }`.
     Прелюдия: схлопнуть пробелы, пробел после `:` внутри скобок условий.
   - `to_source()` — текст для каскада: `raw` у нетронутых узлов, регенерация у изменённых/новых.
   - Мутации: `insert_rule(parent, index, text)` (ошибки IndexSize/Syntax/HierarchyRequest: `@import` не после других
     правил, `@namespace` и т.д.; в стилевом/группирующем правиле допустима одиночная декларация → `NestedDeclarations`,
     SyntaxError только если нет ни одной валидной), `delete_rule`, `set_prelude` (selectorText/conditionText/…),
     `set_decls`.
2. `Stylesheet` получает поле `cssom: OnceLock<CssomTree>` (лениво из `source`, не влияет на `PartialEq`; обновить
   `Default`/`Clone`/`merge_from`). После мутации дерева — пересобрать типизированные коллекции из `tree.to_source()`
   (`parse`), СОХРАНИВ `source` исходным; дерево сохранить. `@mixin` в дереве — узел с `raw`; существующие нативы
   `_lumen_stylesheet_mixin_*` продолжают работать по устаревшему индексу (порядковый номер верхнего `@mixin` ↔ тег
   `TopLevelRuleKind::Mixin`), после правки результата mixin — обновить `raw` узла из `MixinRule::css_text()`.
3. Синхронизация каскада: новая операция журнала `CssomOp::TreeEdit` (только тикает epoch/dirty/relayout); в
   `patched_cascade` узлы с правками дерева обрабатываются подменой диапазона `[start,end)` в `sheet.source()` каскада на
   `tree.to_source()` и полным `parse` (несколько узлов — с конца к началу). «Нелоцируемые» узлы вливаются через
   `merge_from(entry.sheet)` как сейчас (их типизированные коллекции уже пересобраны).
4. Нативы (общие для владельческих и конструируемых листов, аргумент `constructed: bool`): `_lumen_cssom_children`,
   `_lumen_cssom_rule_json`, `_lumen_cssom_insert`, `_lumen_cssom_delete`, `_lumen_cssom_set_style`,
   `_lumen_cssom_set_prelude`. Старые `_lumen_stylesheet_{rule_json,media_child_*,insert_rule,delete_rule,rule_set_style,
   media_child_set_style,rule_insert_apply}` и `_lumen_constructed_{rule_json,media_child_*}` — заменить; mixin-нативы оставить.
5. JS: иерархия `CSSRule → CSSGroupingRule → CSSConditionRule → CSSMediaRule/CSSSupportsRule/CSSContainerRule`;
   `CSSStyleRule extends CSSGroupingRule` (`CSSStyleRule.__proto__ === CSSGroupingRule`), `CSSLayerBlockRule`,
   `CSSLayerStatementRule`, `CSSScopeRule` (`start`/`end`, не CSSConditionRule), `CSSStartingStyleRule`,
   `CSSNestedDeclarations`, `CSSImportRule` (+`layerName`), `CSSNamespaceRule`, `CSSFontFaceRule`, `CSSKeyframesRule`/
   `CSSKeyframeRule`, `CSSPageRule`, `CSSPropertyRule`, `CSSCounterStyleRule`, `CSSFontFeatureValuesRule` и т.п.
   Кэш обёрток по `(владелец листа, id)` через `Map`+`WeakRef` (идентичность, `parentRule`), так же кэш обёрток листов.
   `cssRules`/`rules` у групп, `insertRule(text, index=0)`/`deleteRule` на группирующих правилах, `selectorText`-сеттер.
6. Тесты: Rust-юнит в css-parser (разбор/сериализация/мутации), JS-тесты в `crates/js/src/dom/tests/`, затем WPT:
   `python tests/wpt/...` по `docs/wpt-*` (см. `docs/commands.md`) для трёх файлов критерия + `css-nesting`,
   `css-cascade`, `cssom`. Обновить `CAPABILITIES.md`, `ROADMAP.md` (CSSOM-10 → done), `BUGS.md`/`BUGS-FIXED.md` (BUG-1455),
   `docs/subsystems/css-parser.md`/`js.md`.

## Порядок работы (предложение)

1. `cssom_tree.rs`: модель + разбор + сериализация + юнит-тесты (без интеграции).
2. Мутации + `to_source` + юнит-тесты.
3. Поле в `Stylesheet`, пересборка коллекций, `CssomOp::TreeEdit`, `patched_cascade`.
4. Нативы + JS-классы; починить старые тесты (формат `@media` cssText меняется на многострочный, пустое правило
   `sel { }`) — менять ожидания осознанно, по спеке/Chrome.
5. WPT-прогон, доки, `/lumen-task-finish`.

## Проверка

`cargo test -p lumen-css-parser`, `cargo test -p lumen-js --features v8-backend cssom` (см. `docs/commands.md` и память
про холодный слот V8), WPT-файлы критерия.

## Уточнения проекта (сессия 2, разведка завершена, кода по-прежнему нет)

Прочитано целиком: `parser.rs` (Stylesheet, insert_rule/delete_rule, replay_cssom_ops), `install/stylesheets.rs`,
`install/constructed_stylesheets.rs`, `sheet_sync.rs`, JS 12090–12670 в `web_api_shim_mid.js`, три WPT-файла критерия,
`css-nesting/serialize-group-rules-with-decls.html`, `nested-declarations-cssom.html`. Решения, меняющие проект выше:

1. **Дерево — единственный авторитет для ВСЕХ CSSOM-правок** (и верхнеуровневых тоже). Старые `CssomOp::{InsertRule,
   DeleteRule,SetRuleStyle,SetMediaChildStyle}` не использовать для правок дерева: индексы типизированных коллекций
   (`top_level_order`) — «плоские» (вложенные правила разворачиваются в соседние `Style`-теги, at-правила кроме
   media/mixin без тегов), а индексы дерева — иерархические; отобразить одни в другие нельзя. Остаются только
   mixin-операции (`SetMixinResultStyle`, `InsertRuleBodyApply`) по устаревшему индексу.
2. **Синхронизация каскада** = только `CssomOp::TreeEdit`: узел с такими правками → подмена байтового диапазона его
   текста в `source()` каскада на `tree.to_source()` + один полный `parse` (все узлы, с конца к началу). Для
   конструируемых листов (`ConstructedStylesheet.sheet` идёт в каскад через `merge_from(clone)`) типизированные
   коллекции листа пересобираются сразу после правки дерева: `parse(tree.to_source())` с СОХРАНЕНИЕМ прежнего `source`
   (нужен `Stylesheet::set_source_keep`/аналог в parser.rs — мутаторы коллекций разрешены только там).
   Быстрый путь: вставка в КОНЕЦ корня при пустом `mixin_rules` → `merge_from(parse(text))` без полной пересборки
   (CSS-in-JS вставляет тысячи правил; полная пересборка O(n) на вставку была бы O(n²)).
3. **Дерево живёт в `Stylesheet`** (`cssom: OnceLock<CssomTree>`, лениво из `source`; для чтения `&self`, для правки
   `OnceLock::get_mut` после `get_or_init`). Методы CSSOM (children/rule_info/insert/delete/set_style/set_prelude)
   — на `Stylesheet`, поэтому нативы владельческих и конструируемых листов делят код (`constructed: bool` + ключ листа).
   Адресация узла — глобальный `id: u32` (AtomicU32); JS кэширует обёртки `Map<id, WeakRef>` на лист → идентичность
   (`cssRules[0].cssRules[1].parentRule === cssRules[0]`). `rule_json(id)` обязан отдавать `parentId`.
4. **Формат `cssText` (Chrome), подтверждён WPT**: стилевое без детей — `sel { a: b; c: d; }` (декларации в одну
   строку через пробел; пустое — `sel { }`); с детьми — `sel {\n  a: b; c: d;\n  <child>\n  <child>\n}` (строка
   деклараций только если они есть; у ребёнка отступ ставится ТОЛЬКО перед первой строкой, внутренние переводы строк
   не сдвигаются); группа (`@media/@supports/@scope/@container/@layer {}`) — `@media x {\n  <child>\n}`, пустая —
   `@media x {\n}`; `CSSNestedDeclarations.cssText` — `--x: 1; --y: 1;` (без скобок); `CSSStyleRule` вложенный
   сохраняет `&`: `& .b { … }`. Группа внутри стилевого правила держит декларации как ребёнка-NestedDeclarations.
   Пример: `div {\n  @media screen {\n  color: red; background-color: green;\n}\n}` (внутренние `\n` не сдвигаются).
5. **Разбор**: контексты — лист (только правила), тело стилевого правила / группы внутри него (декларации и правила
   вперемешку: декларации до первого правила → `decls` родителя, после правил → узлы NestedDeclarations, подряд идущие
   сливаются в один), декларационные at-правила (`@font-face/@page/@property/@counter-style/@keyframes`-кадры).
   Конец текста закрывает все открытые блоки (WPT «missing closing braces»). `custom-property: {…};` — декларация.
   Валидность селектора: `is_valid_selector_list` для верхнего уровня; вложенное правило — убрать ведущий комбинатор,
   при наличии `&` принимать (WPT `.a { :is(!& .foo, .b) {…} }` сериализуется как есть), иначе тот же валидатор;
   `% {}` → SyntaxError. `insertRule` в стилевом/групповом правиле: одиночная декларация → NestedDeclarations
   (SyntaxError, только если ни одной валидной; пример `nothing-to-insert-because-invalid-property: 2;`).
6. Порядок работы прежний (шаги 1–5 выше). Шаг 1 — `cssom_tree.rs` (+ подмодули `parse`/`serialize`) и юнит-тесты на
   формат из п.4; зарегистрировать `pub mod cssom_tree;` в `lib.rs`, не забыть `#![deny(missing_docs)]`-совместимость.
