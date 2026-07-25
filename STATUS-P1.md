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
- [ ] **S6 — diff-механизм для DOM-мутаций + ужать инвалидацию.** Следующий
      шаг: научить `bind_model` (и отдельно JS-мутации на странице) репортить,
      какие узлы реально изменились, чтобы `restyle_root_set_for_node_change`
      можно было применять не только к чистым interactive-state переходам —
      это разблокирует CC12_KEY и любой реальный ввод текста. Если после этого
      пол всё ещё упирается в неустранимую стоимость — вернуться с числами и
      пересмотреть бюджет CC-12 (НЕ ослаблять гейт молча).

## Заблокировано до завершения BUG-341 (брать только после S5-зелёного)

ROADMAP.md:602
ROADMAP.md:603
