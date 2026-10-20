# CSSOM-10 — CSSOM для at-правил и вложенных правил (заметка передачи, P1)

Ветка `p1-cssom-10`, слот `p1-work`. Строка задачи — `ROADMAP.md` (CSSOM-10), дефект — `bugs/BUG-1455-OPEN.md`.
Критерий готовности: `tests/wpt/css/css-cascade/layer-cssom-order-reverse.html`, `css-nesting/cssom.html`,
`css-cascade/scope-cssom.html` зелёные (+ таблица проб из BUG-1455: `insertRule` для всех at-правил, `cssRules` их видит).

## Состояние

Сессия 3: шаги 1–2 готовы и закоммичены (0232f9fcb): `crates/engine/css-parser/src/cssom_tree.rs` + подмодули
`parse/serialize/mutate/tests` (25 юнит-тестов зелёные, clippy чистый). Следующая сессия начинает с шага 3 (см. «Сессия 3» внизу).

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

## Сессия 3 — что сделано и что дальше

**Сделано (шаги 1–2).** `CssomTree::{parse(src), get, parent_of, visible_children_of, to_source, css_text(id), insert_rule(parent,text,index),
delete_rule(parent,index), set_prelude(id,text)->Result<bool>, set_decls(id,decls), set_raw(id,raw)}`; `CssomTreeError::{IndexSize,Syntax,
HierarchyRequest,NotFound}`; `RuleKind` (+`js_class()`, `visible()`, `is_grouping()`, `body()`); узел `CssomNode{id,kind,prelude,decls,children,raw}`.
`Opaque` — скрытый узел (неизвестные at-правила, `@function` и т.п.) живёт в дереве ради `to_source`, но не виден в `cssRules`; индексы
insert/delete — по ВИДИМЫМ детям. `Mixin` — видимый узел с `raw`, `css_text` берёт из `parse(raw).mixin_rules[0].css_text()`.
Мелочь, сделанная в других файлах: `render_container` в `parser/mixins.rs` стал `pub(crate)`; в `parser/selectors.rs` добавлены
`canonical_selector_list(text, nested)` (подмена `&` на `:is(.lumen-nesting-amp)` на время разбора) и `split_selector_commas`.
Формат `cssText` реализован по п.4 «Уточнений»; `@import 'x'`/`@namespace` нормализуются в `url("x")`.

**Шаг 3 (следующий) — детали, выясненные чтением кода:**
- `Stylesheet` (`parser.rs:156`) — ручные `Default/Clone/PartialEq`, `merge_from` деструктурирует ВСЕ поля: добавить поле
  `cssom: CssomCell` (новый тип в cssom_tree.rs: обёртка над `OnceLock<CssomTree>`, `PartialEq` всегда true, `Clone` клонирует дерево,
  `Default`), в `merge_from` — `cssom: _` и сброс `self.cssom` (дерево устарело). Дерево лениво строится из `source` (None → пустое).
- Методы на `Stylesheet` (в `parser.rs` или новом `parser/cssom_api.rs`; прямые push/extend в коллекции вне parser.rs запрещены gate-тестом):
  `cssom_tree()`, `cssom_insert_rule(parent,text,index)`, `cssom_delete_rule`, `cssom_set_prelude`, `cssom_set_decls`. После успешной правки
  — пересборка типизированных коллекций: `let mut f = parse(&tree.to_source()); f.source = self.source.clone(); f.cssom = <дерево>; *self = f`
  (source ОБЯЗАН остаться исходным: `entry_is_current` в `sheet_sync.rs` сравнивает его с текстом `<style>`). Быстрый путь: вставка в конец
  корня при пустом `mixin_rules` → `merge_from(parse(node_source))` (CSS-in-JS вставляет тысячи правил).
- Мостик для mixin-нативов (`_lumen_stylesheet_mixin_*`, они принимают `rule_idx` = индекс в `cssom_rules()`, т.е. в `top_level_order`):
  нужен `Stylesheet::mixin_cssom_index(tree_id)` = позиция k-го тега `TopLevelRuleKind::Mixin` в `top_level_order`. После правки mixin-результата
  обновлять `tree.set_raw(id, MixinRule::css_text())` и писать `TreeEdit`. Тест `mixin-invalidation.tentative.html` (3 подтеста: правка `.style`
  внутри `@result` и `ss.cssRules[1].insertRule('@apply --m3;')` в стилевое правило) должен остаться зелёным/не хуже — `@apply` в теле
  стилевого правила в дереве = декларация-маркер `MIXIN_APPLY_MARKER` (в `decls` или в `NestedDeclarations`); старая нативка
  `insert_rule_body_apply` вставляет по индексу среди маркеров.
- Синхронизация (`crates/js/src/v8_runtime/sheet_sync.rs::patched_cascade`): добавить `CssomOp::TreeEdit` (в `parser/cssom_op.rs`; `replay_cssom_ops`
  его игнорирует). Для узла с TreeEdit: `locate_embedded_source` даёт `(start,end)` текста листа в `sheet.source()` каскада → подменить
  `[start,end)` на `entry.sheet.cssom_tree().to_source()`, несколько листов — по убыванию `start`, затем ОДИН `parse` результата; старые
  `bases`/`replay` для таких узлов не применять. «Нелоцируемые» листы (пустой `<style>` + `insertRule`) вливаются `merge_from(entry.sheet)` как сейчас.
  Старые `record(InsertRule/DeleteRule/SetRuleStyle/SetMediaChildStyle/...)` в нативах заменить на `record(node, CssomOp::TreeEdit)`.
- `install/stylesheets.rs` (430 строк, прочитан) и `install/constructed_stylesheets.rs` (294) — нативы шага 4; реестр листа:
  `Arc<Stylesheet>` в `StylesheetNodeEntry`, правка через `Arc::make_mut`. Конструируемые листы идут в каскад через `merge_from(clone)`.
- JS-часть (шаг 4): `web_api_shim_mid.js` ~12090–12670, `insertRule` листа ~12458, конструируемый ~12615, `CSSGroupingRule.insertRule` для @apply ~12246.

**Проверка шага 3:** `cargo test -p lumen-css-parser`, затем `every_stylesheet_mutation_in_the_workspace_announces_itself` (в составе), clippy.


## Сессия 4 — что сделано и что дальше

**Сделано (шаг 3a, css-parser, закоммичено WIP; `cargo test -p lumen-css-parser` — 628 зелёных).**
- `cssom_tree.rs`: `CssomCell` (ленивое дерево, `Clone` клонирует дерево, `PartialEq` всегда true), `CssomTree::node_source(id)`.
- `Stylesheet.cssom: CssomCell` (Default/Clone/merge_from/sheet_diff обновлены; `merge_from` сбрасывает дерево).
- `parser/cssom_api.rs`: `cssom_tree()`, `cssom_insert_rule(parent,text,index)->Result<usize,CssomTreeError>` (быстрый путь: вставка в конец корня при пустых `mixin_rules` → `merge_from(parse(node_source))`), `cssom_delete_rule`, `cssom_set_prelude`, `cssom_set_decls`, `mixin_cssom_index(tree_id)`, `sync_mixin_node(tree_id)`, `cssom_source_text()`; пересборка `rebuild_from_tree` сохраняет `source`. 6 юнит-тестов там же.
- `CssomOp::TreeEdit` добавлен (replay его игнорирует).

**Дальше (шаг 3b → 4 → 5), решения уже приняты:**
1. `crates/js/src/v8_runtime/sheet_sync.rs::patched_cascade`: для узла с `TreeEdit` в журнале — собрать правки-подмены `(start,end,text)`: найденный лист → `[start,end)` ← `entry.sheet.cssom_source_text()`; пустой `<style>` (needle пуст) с деревом, содержащим `Layer`/`LayerStatement` (порядок слоёв зависит от места) → вставка в `cursor` (позиция после предыдущего найденного листа), остальные пустые — прежний `merge_from` в конец. Подмены применять по убыванию `start` к `sheet.source()` каскада, один `parse`, затем `carry_color_profile_data(sheet)`. Нет `source()` → прежний путь. Старый replay-путь оставить для совместимости. Критерий `layer-cssom-order-reverse.html` как раз вставляет `@layer second {}` в ПЕРВЫЙ пустой `<style>`.
2. Нативы (новый файл `install/cssom_tree_natives.rs`, подключить в `install/mod.rs`, вызвать там же, где `install_stylesheets`/`install_constructed_stylesheets`): общие для владельческих и конструируемых листов, первые аргументы `constructed: bool, idx: u32`: `_lumen_cssom_children(…, parent|0=root) -> Vec<u32>`, `_lumen_cssom_rule_json(…, id)` (поля: id, parent(0=корень), cls=`RuleKind::js_class()`, prelude, cssText, styleCssText, childCount; для Mixin — name/cssText из `mixin_rule_json`), `_lumen_cssom_insert(…, parent, text, index) -> i32` (≥0 индекс; -1 IndexSize, -2 Syntax, -3 HierarchyRequest, -4 NotFound), `_lumen_cssom_delete`, `_lumen_cssom_set_style(…, id, text)` (через `parse_inline_style` + `cssom_set_decls`), `_lumen_cssom_set_prelude(…) -> i32`. Для владельческих листов после успеха `sync.record(node, CssomOp::TreeEdit)`; для конструируемых достаточно новой ревизии. Старые `_lumen_stylesheet_{rule_json,media_child_*,insert_rule,delete_rule,rule_set_style,rule_insert_apply}` и `_lumen_constructed_{rule_json,media_child_*,insert_rule,delete_rule}` заменить; mixin-нативы (`_lumen_stylesheet_mixin_*`) оставить, но принимать id узла дерева → `mixin_cssom_index`, после правки `sync_mixin_node(id)` + `TreeEdit`. Нужен `CssomTree::decls_text(id)` (строка деклараций через `serialize::decls_line`, `@apply` как маркер).
3. JS (`crates/js/src/shim/web_api_shim_mid.js` 12090–12680 + `_lumen_make_rule_style` ~3867 и ветка `__loc__` в `_lumen_style_get_parsed/set_parsed` ~3561): переписать на дерево (иерархия классов и кэш обёрток `Map<id, WeakRef>` на лист — см. п.5 «Проект»); `loc` для `.style` → `{tree:true, constructed, sheet, id}`; `CSSStyleRule.insertRule('@apply …')` идёт обычным `insertRule` дерева (проверить `mixin-invalidation.tentative.html`).
4. Тесты, которые нельзя сломать, и проверка — раздел «Как устроено сейчас» выше; затем WPT для трёх файлов критерия, доки (`CAPABILITIES.md`, `ROADMAP.md` CSSOM-10 → done, `BUGS.md`/`BUGS-FIXED.md` BUG-1455, `docs/subsystems/css-parser.md`/`js.md`), `/lumen-task-finish`.

## Сессия 5 — что сделано и что дальше

**Сделано (WIP, `cargo check -p lumen-js --features v8-backend` зелёный; JS ещё НЕ переписан — рантайм сейчас сломан, пока не сделан шаг 4-JS).**
- Шаг 3b: `crates/js/src/v8_runtime/sheet_sync.rs::patched_cascade` — для узла с `CssomOp::TreeEdit` текст `[start,end)` в `source()` каскада
  заменяется на `entry.sheet.cssom_source_text()` (с конца к началу, один `parse`, `carry_color_profile_data`); пустой `<style>` с `@layer`
  в дереве (`declares_layers`) вставляется в `cursor`; остальные пустые — прежний `merge_from`. Старый replay-путь (`bases`) оставлен только
  для листов без `TreeEdit` (после замены нативов он не используется).
- Нативы: новый `install/cssom_tree_natives.rs` (подключён в `install/mod.rs` и `v8_runtime.rs` перед `install_stylesheets`):
  `_lumen_cssom_children(c,idx,parent)->Vec<u32>`, `_lumen_cssom_rule_json(c,idx,id)->Option<String>` (поля `id,parent,cls,prelude,cssText,styleCssText,childCount`, у
  Mixin ещё `name,mixinIndex`), `_lumen_cssom_insert(c,idx,parent,text,index)->i32` (≥0 индекс; -1 IndexSize, -2 Syntax, -3 HierarchyRequest, -4 нет узла),
  `_lumen_cssom_delete(c,idx,parent,index)->i32`, `_lumen_cssom_set_style(c,idx,id,text)->i32`, `_lumen_cssom_set_prelude(c,idx,id,text)->i32` (1 применено, 0 игнор, <0 ошибка).
  `c: bool` = конструируемый лист; `parent/id = 0` — корень. Старые `_lumen_stylesheet_{rule_count,rule_json,media_child_*,insert_rule,delete_rule,rule_set_style,rule_insert_apply}`
  и `_lumen_constructed_{rule_count,rule_json,media_child_*,insert_rule,delete_rule}` УДАЛЕНЫ из Rust (JS их ещё вызывает!). Остались `_lumen_stylesheet_{owner_nids,document_indices,index_for,disabled,mixin_*}`
  и `_lumen_constructed_{replace_sync,disabled,set_disabled}`; `_lumen_stylesheet_mixin_set_node_style` теперь после успеха делает `sync_mixin_node` + `TreeEdit`
  (`rule_idx` = индекс в `cssom_rules()` = `mixinIndex` из rule_json).
- css-parser: `CssomTree::decls_text(id)`, `Stylesheet::mixin_tree_id(cssom_idx)`.

**Дальше (шаг 4-JS, затем 5).**
1. `crates/js/src/shim/web_api_shim_mid.js` ~12110–12640 (от `function CSSStyleSheet(options)` до `// Sentinel scope key for document.adoptedStyleSheets`) переписать на дерево.
   Проект: объект-«бэкенд» листа `be={c: constructed, idx: ()=>текущий индекс реестра или -1 (для владельческого — через ownerNid/_lumen_stylesheet_index_for), rules: Map<id,WeakRef>, sheet}`;
   кэш обёрток листов (`Map ownerNid|constructedIdx -> WeakRef`) и правил (`_lumen_css_rule_for(be,id)`: JSON → `Object.create(класс.prototype)` + скрытое `__r={be,id,last}`),
   чтобы `cssRules[0]===cssRules[0]` и `parentRule===cssRules[0]`. Геттеры на ПРОТОТИПАХ (idlharness), живые (каждый раз `_lumen_cssom_rule_json`), удалённое правило → `parentStyleSheet/parentRule = null`, `cssText` из `last`.
   Иерархия: `CSSRule → CSSGroupingRule(cssRules,insertRule(text,index=0),deleteRule) → CSSConditionRule(conditionText) → CSSMediaRule(media)/CSSSupportsRule/CSSContainerRule(containerName,containerQuery)`;
   `CSSStyleRule extends CSSGroupingRule` (`CSSStyleRule.__proto__===CSSGroupingRule`; selectorText get/set через `_lumen_cssom_set_prelude`, style); `CSSLayerBlockRule(name)`, `CSSLayerStatementRule(nameList)`,
   `CSSScopeRule extends CSSGroupingRule` (start/end: `null` если нет; из prelude `(.a) to (.b)`), `CSSStartingStyleRule`, `CSSNestedDeclarations(style)`, `CSSImportRule(href,media,layerName,supportsText,styleSheet=null)`,
   `CSSNamespaceRule`, `CSSFontFaceRule`, `CSSKeyframesRule(name,cssRules,appendRule,deleteRule,findRule)`, `CSSKeyframeRule(keyText,style)`, `CSSPageRule`, `CSSPropertyRule`, `CSSCounterStyleRule`, `CSSFontFeatureValuesRule`,
   `CSSMixinRule` (cssRules/@result через старые `_lumen_stylesheet_mixin_*` с `rule_idx = data.mixinIndex`; только владельческие листы). Константы `CSSRule.*_RULE`, `type` по классу.
   `.style` — `_lumen_make_rule_style({tree:true, be, id})`; в `_lumen_style_get_parsed/set_parsed` (~3561) ветка `loc.tree` (get — `styleCssText` из `_lumen_cssom_rule_json`, set — `_lumen_cssom_set_style`), ветки `loc.child`/старого `rule` удалить, `mixinPath` оставить.
   Сообщения DOMException — как в старом коде («Failed to execute 'insertRule' on 'CSSStyleSheet': …», SyntaxError/IndexSizeError/HierarchyRequestError).
   Другие вызовы: `web_api_shim_mid.js:10608` (`ShadowRoot.styleSheets`) и `web_api_shim_tail_b.js:2268` используют `_lumen_make_css_style_sheet(idx)` — сигнатуру сохранить.
2. Починить тесты (`crates/js/src/dom/tests/v8_cssom_stylesheets.rs`, `v8_bug493_sheet_registry.rs`, `v8_cssom5_constructed_stylesheets.rs`, `v8_bug1130_shadow_root_members.rs`, `crates/js/tests/cases/bug518_mixin_cssom.rs`): формат `@media` cssText теперь многострочный, пустое правило `sel { }`; ожидания менять осознанно (по Chrome/WPT).
   Проверить `mixin-invalidation.tentative.html`: `insertRule('@apply --m3;')` в стилевое правило идёт обычным деревом (старый `insert_rule_body_apply` не вызывается).
3. Юнит-тест на `patched_cascade` с `TreeEdit` (замена диапазона, пустой `<style>` с `@layer`), `cargo test -p lumen-css-parser`, `cargo test -p lumen-js --features v8-backend cssom`, clippy.
4. WPT: `css-cascade/layer-cssom-order-reverse.html`, `css-nesting/cssom.html`, `css-cascade/scope-cssom.html` (+ `nested-declarations-cssom*`, `serialize-group-rules-with-decls`, `layer-rules-cssom`, `idlharness`).
5. Доки (`CAPABILITIES.md`, `ROADMAP.md` CSSOM-10 → done, `BUGS.md`/`BUGS-FIXED.md` BUG-1455, `subsystems/css-parser.md`/`js.md`), `/lumen-task-finish`. Сборка: `cargo check -p lumen-js --features v8-backend` работает в основном каталоге (холодный слот не нужен).

## Сессия 6 — что сделано и что дальше

**Сделано (WIP).** Шаг 4-JS: `web_api_shim_mid.js` (~12090–13000) переписан на дерево — бэкенд листа `be` (`_lumen_css_backend`),
кэш обёрток `Map<id, WeakRef>` (`_lumen_css_wrap`), иерархия `CSSRule → CSSGroupingRule → CSSConditionRule/…`, `CSSStyleRule extends
CSSGroupingRule`, все at-правила (`CSSLayer*`, `CSSScopeRule`, `CSSImportRule`, `CSSNamespaceRule`, `CSSKeyframes*`, `CSSPropertyRule`, …),
методы листа на `CSSStyleSheet.prototype` (общие для владельческих и конструируемых), `_lumen_style_get/set_parsed` с веткой `loc.tree`.
`mixin`-нативы оставлены (`CSSMixinRule.cssRules` только для владельческих листов). Новый файл тестов
`crates/js/src/dom/tests/v8_cssom10_tree.rs` (7 тестов; один ожидал неверную строку — исправлено, перепроверить).
`cargo test -p lumen-js --features v8-backend --lib cssom` — зелёный кроме правленого теста `v8_cssom_stylesheets::
top_level_rule_insert_rule_adds_a_nested_rule_and_rejects_garbage` (теперь вложенное правило вставляется).

**Дальше.**
1. Перегнать `cargo test -p lumen-js --features v8-backend --lib cssom` и `--test all bug518` (интеграционный `crates/js/tests/cases/bug518_mixin_cssom.rs`,
   `mixin-invalidation`), `v8_bug493`, `v8_bug1130`; чинить ожидания осознанно (по Chrome/WPT).
2. Юнит-тест на `patched_cascade` с `TreeEdit` (замена диапазона, пустой `<style>` с `@layer`).
3. WPT: `css-cascade/layer-cssom-order-reverse.html`, `css-nesting/cssom.html`, `css-cascade/scope-cssom.html` (+ `nested-declarations-cssom*`,
   `serialize-group-rules-with-decls`, `layer-rules-cssom`, `idlharness`, `cssom/*`). Не забыть: `.style` на `CSSStyleRule` — PutForwards (сеттер есть).
4. `cargo clippy -p lumen-css-parser -p lumen-js --features lumen-js/v8-backend --all-targets -- -D warnings`; доки (`CAPABILITIES.md`, `ROADMAP.md`
   CSSOM-10 → done, `BUGS.md`/`BUGS-FIXED.md` BUG-1455, `subsystems/css-parser.md`/`js.md`), `/lumen-task-finish`. Сборка идёт в основном каталоге; sccache
   иногда падает «io error» — повторить.


## Сессия 7 — что сделано и что дальше

**Сделано (WIP).** `cargo test -p lumen-js --features v8-backend --lib cssom` (77) и `--test all bug518` зелёные; `cargo test -p lumen-css-parser` — 630 зелёных.
- `sheet_sync.rs`: `SheetSync.child_list_gen` — смена `DomTouched::child_list_gen` у `<style>` при том же тексте (`innerHTML = тот же текст`) пересоздаёт лист и выбрасывает CSSOM-правки (WPT `cssom.html` «subrules 9» зависел от протёкшего состояния).
- css-parser: `parser.rs` — вложенное правило с типовым селектором без `&` (`div {}` в теле стилевого правила; `ident_led_block_ahead`) и prelude с `&` не в голове (`.stuff, :is(&) #inner2`; `parse_nested_rule_with_inner_amp`) теперь раскрываются; `canonical_selector_list` — неявный `& ` у вложенных селекторов без `&` / с ведущим комбинатором (`& > p`) и сохранение при прощающем разборе правила с `&` (`:is(!& .foo)`); `cssom_tree/mutate.rs` — `insertRule` деклараций бросает `SyntaxError`, если нет ни одной известной (`SUPPORTED_PROPERTIES`/`--*`/`-vendor-*`), невалидные среди валидных отбрасываются; `serialize.rs` — пустые `CSSNestedDeclarations` не попадают в `cssText` внешнего правила.
- Тест `bug518_mixin_cssom` обновлён (теперь `@layer` — видимое `CSSLayerBlockRule`); добавлены `outer_selector_text_setter_*`, `script_made_style_*` (v8_cssom_stylesheets.rs), 2 теста вложенности в `parser/tests/nesting.rs`.

**Дальше.**
1. Пересобрать `cargo build --profile dev-release --bin lumen` и прогнать WPT (`.tmp/venv-wpt/Scripts/python.exe`, `MSYS_NO_PATHCONV=1 LUMEN_PROFILE=dev-release … tests/wpt/run_report.py --binary <lumen.exe> --all --root css/css-nesting --log-raw .tmp/c10-raw.log`; разбор падений — `python .tmp/c10_fails.py .tmp/c10-raw.log cssom`).
   Критерий: `css-nesting/cssom.html` целиком (осталось проверить после правок: «Mutating the selectorText…», «Manipulation of nested declarations…»), `css-cascade/layer-cssom-order-reverse.html` (падает «Insert layer invalidates style» — 3 из 4), `scope-cssom.html` (13/13 уже).
   Не сделано: `CSSNestedDeclarations.style.length` возвращает `''` — у `CSSStyleDeclaration.prototype` нет геттера `length`/`item()`/индексов (см. `web_api_shim_mid.js` ~3595, `_lumen_make_rule_style` ~3856; проверить, есть ли они у `element.style`, `v8_gap_rule_cssom.rs:108` читает `d.style.length`).
2. После прогона — `--update-expected` для затронутых `.ini` (`css-nesting`, `css-cascade`), `UNEXPECTED-PASS` → PASS; проверить, что остальные css-nesting/css-cascade/cssom не регрессировали (раньше 73/117 в css-nesting).
3. Юнит-тест на `patched_cascade` с `TreeEdit`; `cargo clippy -p lumen-css-parser -p lumen-js --features lumen-js/v8-backend --all-targets -- -D warnings`; парсер вложенности мог сдвинуть пиксели → `python graphic_tests/run.py --continue-on-fail`.
4. Доки (`CAPABILITIES.md`, `ROADMAP.md` CSSOM-10 → done, `BUGS.md`/`BUGS-FIXED.md` BUG-1455, `subsystems/css-parser.md`/`js.md`), `/lumen-task-finish`.
