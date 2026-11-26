# BUG-1844 — хит-тест не видел слои, вложенные в обычные блоки: клик по панели портала уходил в слой-затемнение

**Статус:** FIXED 2026-11-26 — хит-тест собирает слои объемлющего stacking-контекста одним обходом и ставит их по правилам CSS 2.1 Appendix E; на портале клики по панели попадают в её элементы.
**Заведён:** 2026-11-26 (ZCode, задача «portal.nextera.ru: падение по «Войти», зависание клика, отрисовка полей, клик по панели»)
**Компонент:** paint ([`hit_test.rs`](../crates/engine/paint/src/hit_test.rs))

## Симптом

`https://portal.nextera.ru/` после входа, открытая левая панель:

- `aside#ppSidePanel` — `position: fixed; z-index: 1600`, лежит в `#root > .contents-mount > .contents-mount`
  (оба `display: contents`);
- `.pp-scrim` — слой-затемнение `position: fixed; inset: 0; z-index: 1500`, прямой ребёнок `body`.

`document.elementFromPoint` в четырёх точках по вертикали панели (x = 180, y = 60…700) и в (500, 400)
возвращал `.pp-scrim`; клик автоматизации
отвечал `Element click intercepted: point (180, 266) hits <div> (node 1298)` — а сам путь перехвата к тому же
вешал окно ([BUG-1842](BUG-1842-FIXED.md)). Отрисовка при этом верная: панель видна над затемнением.

## Причина

`hit_test.rs` делил детей stacking-контекста на positive / zero / in-flow / negative только среди прямых детей
уровня и входил в обычные (non-positioned) блоки после всех положительных слоёв. Позиционированный слой,
вложенный в такой блок, в объемлющий контекст не поднимался — хотя по CSS 2.1 Appendix E складывается именно
там. Отрисовка это умеет ([`stacking.rs`](../crates/engine/layout/src/stacking.rs), `walk`).

## Исправление

- `Layers` — один обход `collect_layers` до спуска в уровень: `HashMap<указатель на бокс → Vec<Candidate>>`
  для каждого потомка; `of(b)` отдаёт слои, переданные вверх через `b`.
- `classify` ставит и собственные детей уровня, и переданные вверх; `Candidate.chain` хранит промежуточные
  обычные блоки для `path`; `skip`/`skip_below` не дают полному обходу (`hit_test_all`) продублировать слой.
- Пять тестов в `hit_test::tests`: `layer_nested_in_plain_boxes_outranks_a_lower_layer_beside_them` (главный),
  `nested_layer_keeps_the_plain_boxes_in_its_path`,
  `hit_test_all_lists_a_nested_layer_once_above_the_lower_layer`,
  `positioned_auto_layer_nested_in_a_plain_box_is_above_later_in_flow_content`,
  `negative_layer_nested_in_a_plain_box_stays_below_in_flow_content`.

## Проверка

- Юнит-тесты `hit_test`: с правкой 40/40; на прежнем алгоритме 4 из 5 новых падают.
- 2026-11-26, живой портал, два бинарника на одной странице. Точки (180,60), (180,266), (180,400), (180,700)
  — без правки `div.pp-scrim`; с правкой — `div.pp-side-tools < aside#ppSidePanel.sidebar < div.contents-mount
  < div.contents-mount < div#root < body` и элементы строк панели; (500,400) в обоих — `.pp-scrim`.
- Клики с правкой: `#themeToggleBtn` — `html` получил `light-theme`, подпись сменилась на «Светлая тема»;
  «+ Агент» — открылся `div.pp-overlay` (z-index 2000) и закрылся кликом вне бокса; строка агента в точке
  (180, 266) — `html.pp-chat`, 667 → 2208 узлов. Закреплённая панель (`pp-left-pin`: 300 px, z-index 1400,
  без затемнения, контент сдвинут на 300): `elementFromPoint(180, 266)` отдаёт `span.nav-item-name` строки
  агента; кликом в закреплённом состоянии не проверялось.
- Chrome 158, тот же портал, настоящая мышь (`Input.dispatchMouseEvent`) по `#ppHeadLeft` и по (180, 266):
  те же классы `pp-left-open pp-scrim-on` → `pp-chat`, панель закрывается. Число узлов страницы отличается
  на 32 (2176 в Chrome против 2208 в Lumen) — причина не устанавливалась.
- `cargo clippy -p lumen-paint --all-targets -- -D warnings` чисто.

## Связанное

Порядок слоёв на портале после правки: модал `.pp-overlay` (2000) > панель (1600) > `.pp-scrim` (1500);
`elementFromPoint(180, 266)` при открытом модале отдаёт `.pp-overlay`.
