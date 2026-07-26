# STATUS-P1

**ПРОГРАММИСТ 1 — читать первым.** Задачи ниже брать **строго по порядку сверху
вниз, не пропуская**. Каждый следующий срез опирается на предыдущий; перескок
через срез оставит движок в несогласованном состоянии. Один срез = одна сессия
(check-in с пользователем после каждого влитого среза).

Текущая крупная задача — **BUG-341: инкрементальный restyle+layout** (разблокирует
CC-14/CC-15). Ветка-резервация уже существует: `p1-bug341-layout-cache`
(worktree `.claude/worktrees/bug341-layout-cache/`). Если видишь эту ветку в
`git branch` — **продолжай её, не начинай заново**. Полный бриф с архитектурой,
моделью инвалидации и планом верификации:
[`docs/tasks/p1-bug341-incremental-restyle.md`](docs/tasks/p1-bug341-incremental-restyle.md).

## Очередь BUG-341 (по порядку, не пропускать)

- [x] **S1 — дизайн + профиль.** Готово (коммит `ae098fd9`). Профиль доказал:
      каскад 53%, lay_out 35%, build_box 8% → layout-кэш недостаточен, нужен
      инкрементальный каскад. Бриф + BUG-341 обновлены.
- [x] **S2 — persist `CounterMap` + дифф-тест-каркас `incremental == full`.**
      Готово. `layout_measured_hyp_with_counters` возвращает `(LayoutBox,
      CounterMap)`, старая `layout_measured_hyp` делегирует ей (без изменения
      поведения). `CounterMap::styles()` открывает каскад-кэш. Дифф-каркас в
      `incremental.rs` (`full_cascade`/`incremental_cascade`/`assert_cascades_eq`
      + тесты `incr_cascade_matches_full_trivial`/`_interactive_rules`, пока
      тривиально равные). S3 меняет только тело `incremental_cascade`.
- [x] **S3 — инкрементальный каскад, v1 (консервативная инвалидация).**
      Готово. `counters::incremental_precompute_counters` + `RestyleDelta`
      переиспользуют `ComputedStyle` вне dirty root-set; root-set —
      `style::restyle_root_set_for_state_change` (hover/focus/active,
      ancestor-chain-aware) / `style::restyle_root_set_for_node_change` (DOM
      attribute/class). За флагом `INCREMENTAL_RESTYLE` (выкл по умолчанию,
      без него — полный пересчёт). 4 дифф-теста в `incremental.rs`. Замер:
      `precompute_counters` p50 падает ~54% на реалистичном hover-переходе
      между соседними вкладками, ~1% на фикстуре CC-12 (SIDEBAR/None-тумблер
      — задокументированный худший случай, см. BUG-341 «S3»). В пайплайн ещё
      не встроено (это S5).
- [x] **S4 — инкрементальный box-build.** Готово. `build_box_or_reuse`
      (все 4 точки рекурсии `build_box`) клонирует целиком поддерево
      `LayoutBox` из `prev`, если `CounterMap::clean_subtrees` +
      `RestyleDelta::dom_content_stable` подтверждают безопасность (только
      для чисто interactive-state дельт — DOM-мутации консервативно
      пересобираются полностью, как в S3). Точка входа
      `box_tree::incremental_build_box`, флаг `set_incremental_box_build`
      (выкл по умолчанию). 3 дифф-теста (сравнение по геометрии после
      `lay_out`, НЕ по `Debug`-строке — `custom_props: HashMap` не
      гарантирует порядок обхода между независимыми каскадами), все зелёные.
      **Честный замер отрицательный**: `index_by_node` (полный обход/хеш
      предыдущего дерева на каждый вызов) перевешивает экономию от пропуска
      ~8%-й доли `build_box` — см. BUG-341 «S4» для чисел и рекомендации
      перезамерить вместе с S3 на S5, прежде чем решать, включать ли флаг.
- [x] **S5 — включить инкрементальный путь в chrome-пайплайн.** Готово. Новая
      `lumen_layout::box_tree::layout_mutation_incremental_restyle` (S3-каскад
      + существующий `graft_geometry`; S4 box-build-skip сознательно оставлен
      выключенным, см. рекомендацию в S4). `relayout_chrome_host` берёт этот
      путь, когда `ChromeModel` (новый `PartialEq`, все ~20 вложенных типов) не
      изменился с прошлого кадра + viewport/Forced-Colors стабильны — иначе
      полный пересчёт. 2 новых дифф-теста в `incremental.rs`, все зелёные
      (3260 passed / 2 pre-existing FONT_CH_EX fails в lumen-layout). Замер: на
      собственном худшем фикстуре CC-12 (SIDEBAR/None-тумблер) выигрыша нет
      (ожидаемо, см. S3), на репрезентативном hover между соседними вкладками
      — реальные ~25% (85→64мс p50). **Гейт CC-12 всё ещё красный** (~40-45×
      бюджет) — см. BUG-341 «S5» для полных чисел и списка нерешённого.
      Страничный pipeline (JS-мутации, ADR-016 M4) НЕ тронут — там нет
      diff-механизма для DOM-мутаций, тот же блокер, что и для CC12_KEY.
- [x] **S6 — diff-механизм для DOM-мутаций (сторона chrome).** Готово. Новая
      `lumen_chrome::bind_model_tracked` репортит узлы, чей селектор-значимый
      атрибут/класс реально изменился, или чей row-list-контейнер набрал/потерял
      строку — инструментированы общие примитивы (`set_attr`/`remove_attr`/
      `remove_children_with_class`/`reconcile_row_list`), через которые проходят
      все `bind_*`-функции, без протаскивания параметра по функциям.
      `relayout_chrome_host` объединяет `restyle_root_set_for_node_change(doc,
      touched)` с interactive-state root-set'ом и убрал старый гейт по равенству
      всего `ChromeModel`. Найден и исправлен реальный баг: `bind_palette`
      безусловно пересобирал пустой плейсхолдер каждый цикл, перманентно
      расширяя `dirty_roots`. **Замер**: CC12_KEY ~30% p50 (~90→~63мс, первое
      реальное улучшение на этой фикстуре), CC12_HOVER без изменений (ожидаемо
      — S6 не трогает interactive-state root-set). Гейт CC-12 всё ещё красный
      (~40-50× бюджет) — см. BUG-341 «S6». JS-мутации на странице
      (`v8_runtime.rs`) НЕ подключены — нет фикстуры, которая их проверяет,
      и это отдельный, самостоятельный дизайн (другие точки входа).
- [x] **S7 — diff для page-side JS-мутаций + сужение hover fan-out.**
      🟡 **Часть 1 готова**: `lumen_js::v8_runtime::DomTouched` /
      `V8JsRuntime::take_dom_touched()` — трекер по образцу `bind_model_tracked`,
      инструментированы 9 атрибутируемых нативов, ещё 13 помечены `unattributed`
      (откат к полному каскаду). 12 тестов. Детали — BUG-341 «S7 (part 1)».
      ✅ **Часть 2 готова**: `Lumen::try_relayout_raf_incremental` реально берёт
      `layout_mutation_incremental_restyle`, когда `take_dom_touched()` даёт
      атрибутируемую сводку и кэш `page_prev_cascade_styles`
      (`Option<HashMap<NodeId, ComputedStyle>>`) валиден — иначе откат на
      прежний `layout_mutation_incremental`. Кэш инвалидируется по умолчанию в
      едином стоке `apply_relayout_result` (покрывает `relayout()`,
      `try_relayout_raf_incremental`, `readback_relayout_job`,
      `poll_engine_commit`) и явно во всех точках, что его обходят (bfcache
      thaw, `apply_loaded_page`, streaming-layout, hibernate restore) —
      переживает переключение вкладок через `PageSnapshot` синхронно с
      `layout_box`. Новый JS-driven дифф-тест в `lumen-js`
      (`dom_touched_drives_incremental_restyle_matching_full_cascade`): реальная
      V8-мутация `classList.add` → `take_dom_touched()` → `RestyleDelta` →
      результат побайтово совпадает с полным пересчётом. `cargo test -p
      lumen-js --features v8-backend` (2523 passed), `cargo test -p
      lumen-shell` (1704 passed), оба clippy чисты. **Не подключён**
      движковый поток (`submit_relayout_job`/`readback_relayout_job`/
      `poll_engine_commit`) — там `RestyleDelta`/`CounterMap` пришлось бы
      пересылать через границу потока, отдельная задача; поведение там не
      изменилось (по-прежнему полный каскад). `graphic_tests/run.py` не
      удалось прогнать в этой среде — первый кадр DX12-бэкенда занимает
      >12–27с (не связано с этим срезом, симптом совпадает с OPEN BUG-274
      cold-start), 5-секундный таймаут гарпуна не переживает. Детали —
      BUG-341 «S7 (part 2)».
      ✅ **Часть 3 готова (сужение hover fan-out)**: `restyle_state_needs_fanout`
      (новая, `lumen_layout::style`) сканирует стили на селекторы вида
      `:hover`/`:focus`/`:active` + sibling-combinator (`+`/`~`) где-либо на
      пути к subject — только тогда `restyle_root_set_for_state_change`
      по-прежнему расширяет флип до родителя; иначе сужает до самого
      флипнутого узла. `assets/chrome/chrome.html` не содержит ни одного
      такого селектора → реальный chrome-каскад теперь сужается по-настоящему.
      16 новых юнит-тестов на все формы (subject/descendant/sibling,
      `:is()`/`:not()`/`:has()`, `@media`, shadow-root). **Честный замер**:
      A/B на `#sbTabs` (6 табов) не показал измеримой разницы (в пределах
      шума станка) — сужение с «весь контейнер» до «2 реально изменившихся
      таба» не двигает wall-clock на этой маленькой фикстуре; `CC12_HOVER`
      (SIDEBAR/None-тумблер) не затронут по той же причине, что и в S3 (его
      каскад-стоимость уже была близка к нулю). Гейт CC-12 остаётся красным
      (~45-50× бюджета). Реальная польза узкого сужения ожидается на широких
      sibling-структурах (длинный `:hover`-список), которых нет в текущих
      фикстурах — задокументировано как теоретический, не измеренный случай.
      Найден и задокументирован попутный баг **BUG-349** (не починен — вне
      скоупа): `restyle_root_set_for_node_change`'s doc ошибочно утверждал
      «у движка нет `:has()`», хотя `:has()` реализован — DOM-мутация может
      оставить устаревшим `:has()`-зависимый стиль далёкого предка. Детали —
      BUG-341 «S7 (part 3)».
      Если бюджет CC-12 пересматривается вместо продолжения — вернуться с
      числами (НЕ ослаблять гейт молча).

## Заблокировано до завершения BUG-341 (брать только после зелёного гейта CC-12)

ROADMAP.md:602
ROADMAP.md:603
