# BUG-1112 — `ShareCache` никогда не вставляет записи на реальных сайтах: `selector_is_share_safe` запрещает любой комбинатор

**Статус:** OPEN
**Заведён:** 2026-09-23 (P3, при ревизии очереди BUG-935 — блокер найден,
но не зафиксирован отдельным багом THREAD-4 (P1), скоуп которого был
про стоимость матчинга, не про сам критерий безопасности шаринга).
**Область:** layout (`crates/engine/layout/src/style/cascade.rs`,
`crates/engine/layout/src/style/share_cache.rs`).

## Симптом

Style-sharing cache (`ShareCache`, `compute_style_shareable`) существует и
подключён в проде, но на реальных сайтах не вставляет ни одной записи:
инструментация THREAD-4 среза 5 (`p1-thread4-recheck-srez5`,
2026-09-23, временный `lumen_core::profile::scope` на
`share_hit`/`share_miss`/`share_key_some`/`share_insert` в
`share_cache.rs::compute`, не закоммичена) на живом github.com
(`LUMEN_PROFILE_TREE=1 LUMEN_PROFILE_DETAIL=1 --trace-nav`, 3 прохода)
дала `share_hit` = **0** во всех трёх, `share_key_some` (узел прошёл
структурный фильтр `build_key`) стабильно 391, но `share_insert` = **0**
за все три прохода. Кэш формально жив, но пуст всегда — весь механизм
мёртвый вес.

Это же независимо всплыло как объяснение регрессии в BUG-935 срезе 47:
живой A/B `LUMEN_BUG935_M4_SWAP=1` на lenta.ru дал восемь
`(incremental, on-thread)` relayout'ов с `apply_ms` 52–2765мс, во всех
восьми `cascade_reused=0` — «инкрементальный» путь на сайте без единой
записи в `ShareCache` вырождается в полный пересчёт каскада, просто
исполняемый синхронно на UI-потоке вместо движкового, что дало не
увеличенный RTT, а настоящий фриз event loop (MCP-таймаут автоматизации
дважды подряд).

## Корень (по коду)

`selector_is_share_safe` (`cascade.rs:1649`) требует `sel.tail.is_empty()`
— ноль комбинаторов, то есть допускает только «голые» составные
селекторы (`tag`/`.class`/`#id`/`*` без потомков/соседей в цепочке).
Primer (github.com) и подавляющее большинство реальных сайтов
стилизуют почти исключительно через descendant-комбинаторы
(`.foo .bar`, `.card > .title` и т.п.) — под этим критерием такие
правила глобально дисквалифицируют узел из `shareable` (`cascade.rs:858`,
`:901`, `:947`, `:985` — `shareable &= rule.selectors.iter().all(selector_is_share_safe)`),
и `share_insert` не срабатывает никогда.

Причина ограничения — комментарий на `cascade.rs:1641-1648`: комбинатор
делает результат матчинга зависимым от предков/соседей, которых
структурный ключ узла (`build_key`) не captur-ит, так что два узла с
одинаковым ключом могут матчиться по-разному, если их предки различны.
Реальные браузеры (Servo) решают это не запретом комбинаторов, а
сравнением всей цепочки предков (указатель на уже вычисленный
`ComputedStyle` родителя как часть ключа шаринга, рекурсивно) — то есть
архитектурным расширением `ShareCache`, не точечной правкой одной
функции.

## Что сделать

Не мелкая правка. Нужно расширить `ShareCache`/`build_key` так, чтобы
ключ шаринга учитывал идентичность (или эквивалентный структурный ключ)
цепочки предков до нужной глубины комбинатора, и только тогда допускать
селекторы с `tail` непустым. Вероятная форма — по образцу Servo:
хранить в ключе указатель/дайджест на разделяемый стиль родителя,
переиспользовать запись только если у кандидата тот же родительский
разделяемый стиль. Требует:
- расширения `ShareKey`/`build_key` (`share_cache.rs`) цепочкой предков;
- пересмотра `selector_is_share_safe` для допуска `Combinator::Descendant`/`Child`
  при наличии проверяемой идентичности предка;
- полного набора гейтов ([`docs/graphic-tests.md`](../docs/graphic-tests.md))
  — некорректный шаринг стиля даёт молчаливо неверную раскладку, не падение;
- переизмерения BUG-935's M4-своп (`LUMEN_BUG935_M4_SWAP`) на lenta.ru
  после фикса — до этого своп остаётся отклонён по умолчанию.

## Связанные

- [BUG-935](BUG-935-OPEN.md) — M4-инкрементальный rAF-путь блокирован
  этим дефектом (срез 47, живой A/B на lenta.ru).
- THREAD-4 (`ROADMAP.md`, done) — срез 5 нашёл этот корень, срез 7 закрыл
  задачу по своему скоупу (`cs_match`/`RuleIndex`), не трогая
  `selector_is_share_safe`.

## Воспроизведение

Временная инструментация THREAD-4 среза 5 не закоммичена — повторить:
`lumen_core::profile::scope` на входе/выходе `ShareCache::compute`
вокруг `share_hit`/`share_miss`/`share_insert`,
`LUMEN_PROFILE_TREE=1 LUMEN_PROFILE_DETAIL=1 cargo run --profile
dev-release -p lumen-shell -- --trace-nav https://github.com` — `share_insert`
остаётся 0 на любом реальном сайте со стилизацией через комбинаторы.

## Срез 1 (2026-09-23)

Реализована узкая, но полная (не частичная) версия предложенного фикса —
без новой структуры данных. Ключевое наблюдение: `ShareKey.inherited_ptr`
(адрес живой `ComputedStyle`-аллокации родителя) уже даёт нужную гарантию
индукцией, без явного хранения цепочки предков — `counters::walk` вручает
ОДНУ и ту же `Arc`-аллокацию всем детям узла (`crates/engine/layout/src/
counters.rs:1412`), поэтому совпадение `inherited_ptr` у двух узлов
возможно только в двух случаях: (a) это буквально дети одного и того же
живого родителя, либо (b) их родители сами совпали по ключу ShareCache
(что требует их взаимной `is_svg_presentational_element`-пригодности и
рекурсивно того же самого условия на уровень выше). Оба случая означают,
что весь "пригодный" (SVG-presentational) отрезок цепочки предков у двух
узлов совпадает попарно по tag+attrs вплоть до буквально общего DOM-узла —
что делает `Type`/`Class`/`Id`/`Universal`-селектор на любом предковом
compound безопасным при ЛЮБОЙ глубине `Descendant`/`Child`. `+`/`~`
(sibling) остаются под запретом — позиция среди соседей в ключе не
закодирована никак.

Изменено: `selector_is_share_safe` (`cascade.rs`) теперь проверяет КАЖДЫЙ
compound селектора (не только `head`) на simple-parts-only, и разрешает
`Combinator::Descendant`/`Combinator::Child` в `tail` (было: `tail` только
пустой). Doc-комментарии `compute_style_shareable`/`selector_is_share_safe`/
модуля `share_cache.rs` переписаны под новое рассуждение. Новый тест
`a_descendant_selector_still_shares_when_the_key_proves_ancestor_identity`
(`style/tests/share_cache.rs`) прямо демонстрирует выигрыш: `.octicon path`
(descendant-комбинатор) теперь шарит `Arc` между повторными иконками, где
раньше `selector_is_share_safe` глушил это безусловно. Существующий
`a_combinator_rule_disables_sharing_for_the_nodes_it_could_reach` остаётся
зелёным без изменения поведения — там `.blue`/`.plain` сами не
`is_svg_presentational_element`, поэтому их `inherited_ptr` в принципе
никогда не совпадает, комбинатор тут ни при чём — комментарий теста
обновлён, чтобы это не выглядело противоречием.

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`scripts/scoped-test.sh crates/engine/layout/src/style/cascade.rs` — 0
failed по всем засеваемым крейтам; `python graphic_tests/dump_golden.py
--build` — 12/12 дампов (layout + display-list) совпадают с эталоном,
подтверждает нулевой дифф на пяти представительных страницах (включая
table/grid/flex/transform-zindex). Полный пиксельный `graphic_tests/
run.py --continue-on-fail` не прогнан — калибровка `TEST-00` падает в
этой песочнице («захват экрана сломан» — среда не даёт живому окну
реальный фокус, симптом уже описан в `docs/CLAUDE.md`'s «gdigrab из фона»
и не специфичен для этого среза).

**Что НЕ сделано** (следующий срез):
- Эта версия НЕ расширяет `is_svg_presentational_element`-скоуп на
  обычные HTML-узлы — реальный выигрыш на живых сайтах (`share_insert`
  для `.octicon path`-подобных правил) ограничен тем, что уже происходит
  ВНУТРИ уже-пригодного SVG-поддерева. Живой прогон (см. «Воспроизведение»
  выше) на `github.com`/`lenta.ru` не выполнен в этой сессии — в песочнице
  нет сетевого доступа (тот же блокер, что и в BUG-935 срез 46-47).
  Нужно повторить инструментацию THREAD-4 среза 5 и подтвердить
  `share_insert > 0` на реальной странице, прежде чем переизмерять
  [BUG-935](BUG-935-OPEN.md)'s `LUMEN_BUG935_M4_SWAP` на `lenta.ru`.
- Если живой прогон покажет, что основная масса повторов лежит НЕ внутри
  SVG-иконок (а, например, в повторяющихся карточках списка), потребуется
  отдельный срез на расширение eligibility-скоупа — то самое "архитектурное
  расширение", описанное в постановке задачи выше, с полным аудитом
  presentational-hint/quirks-путей для обычного HTML.

## Срез 2 (2026-09-23)

Живой прогон, который срез 1 не смог сделать («нет сетевого доступа»),
оказался возможен в этой сессии — сеть в песочнице есть. Добавлена
измерительная (`LUMEN_SHARECACHE_STATS=1`, по образцу
`LUMEN_BUG935_M4_SWAP`) постоянная инструментация `ShareCache`:
`hit`/`insert`/`miss` (+ диагностический разбор `miss` на `key_none` vs
`key_some_unshareable`) печатаются в stderr по каждому проходу
(`crates/engine/layout/src/style/share_cache.rs`, `Drop for ShareCache`).
Не влияет на поведение при не выставленной переменной (`OnceLock`, читается
один раз за процесс).

Замер (`LUMEN_SHARECACHE_STATS=1 lumen.exe --trace-nav out.json <url>`,
dev-release): **`share_insert` остаётся 0** и на github.com
(`hit=0 insert=0 miss=1780`, из них `key_some_unshareable=391` — узлы,
получившие ключ, но отклонённые как небезопасные), и на lenta.ru
(`hit=0 insert=0 miss=1508`, `key_some_unshareable=81`) — срез 1 не даёт
измеримого выигрыша ни на одном реальном сайте в этой сессии.

## Срез 3 (2026-09-23)

Нашёл и починил ДВА конкретных источника ложного `key_some_unshareable` —
оба через временную адресную инструментацию (`eprintln!` селектора,
отклонившего `shareable`, не закоммичена), а не результат гадания.

**Находка 1 — `:root { --custom-prop: … }` глушит расшаривание для ВСЕХ
391 узлов на github.com.** `RuleIndex::candidates` кладёт любой селектор,
чей subject не имеет type/class/id (а `:root`'s subject — чистый
pseudo-class), в bucket `universal`, который возвращается безусловно для
КАЖДОГО запроса (`rule_index.rs:231`). `selector_is_share_safe` видел этот
кандидат для каждого SVG-узла и безусловно банил pseudo-class — даже
несмотря на то, что `:root` физически не может совпасть ни с одним
`is_svg_presentational_element`-тегом (документный root — всегда
`<html>`). Фикс: `PseudoClass::Root` в SUBJECT-позиции теперь считается
безопасным — жёсткое несовпадение, доказуемое без всякого ключа.

**Находка 2 (после фикса 1) — атрибутные селекторы на subject'е.**
github.com's dark-mode custom properties
(`[data-color-mode=light][data-light-theme*=light] { … }`) — тот же
механизм (`universal`-bucket, subject без type/class/id) душил все те же
391 узла. Но `ShareKey.attrs` УЖЕ пинит ПОЛНЫЙ набор атрибутов subject'а
(см. module doc `share_cache.rs`) — значит любой атрибутный селектор на
subject'е детерминирован по ключу, ровно как `Class`/`Id`. Прежнее
ограничение «`Class`/`Id` и всё» было строже, чем реально требует ключ.
Фикс: `Attribute(_)` в SUBJECT-позиции тоже безопасен.

**Найденный и исправленный ДО коммита баг в собственном фиксе.** Первая
версия обоих фиксов различала subject/ancestor по `sel.head` —
неправильно: `ComplexSelector::head` это ЛЕВЫЙ (самый дальний предок)
compound, а не subject; `matching.rs::matches_complex` матчит `node`
против ПОСЛЕДНЕГО элемента `tail` (или `head`, если `tail` пуст) —
см. `ComplexSelector`'s doc comment (`css-parser/src/parser/selectors.rs:479`)
и `matches_chain` (`matching.rs:53`). С первой версией фикса атрибутный
селектор в ПРЕДКОВОЙ позиции (`[data-theme=b] .octicon { … }`) стал бы
ошибочно считаться безопасным — `ShareKey` не пинит атрибуты предка,
только subject'а, так что это было бы тихой порчей стиля. Пойман до
пуша: 391 у github.com не изменилось после первой версии фикса (должно
было упасть, раз убрали `:root`-блокер), что и привело к перепроверке
логики head/tail. Функция переписана: subject — это `tail.last()` (или
`head` при пустом `tail`), только этому compound-у разрешены
`Root`/`Attribute`; все более ранние compounds остаются на прежнем,
строгом правиле (`Type`/`Class`/`Id`/`Universal`-only).

**Результат живого перемера после обоих фиксов:** `share_insert`
**остаётся 0** и на github.com, и на lenta.ru. Причина —
третий, архитектурно более глубокий источник того же
`universal`-bucket-эффекта: `.pagination > :first-child` /
`.pagination > :last-child` / `.btn .octicon:only-child` — субъектные
pseudo-classes `:first-child`/`:last-child`/`:only-child` ЗАВИСЯТ от
позиции среди соседей, которую `ShareKey` не кодирует ни на каком уровне
(тот же класс, что уже документированный запрет `NextSibling`/
`LaterSibling`) — это ЗАКОННОЕ, не ложное отклонение. Но `universal`-bucket
делает эти правила кандидатами для ЛЮБОГО узла документа независимо от
того, действительно ли он потомок `.pagination`/`.btn`, поэтому все 391
(github) / 79 из 81 (lenta.ru, после фиксов 1-2 упало с 81) SVG-узлов
документа гарантированно отклоняются хотя бы одним из них. Тесты
(`share_cache.rs`): `a_root_scoped_custom_property_rule_does_not_disable_sharing`,
`a_subject_attribute_selector_does_not_disable_sharing`,
`an_ancestor_position_attribute_selector_still_disables_sharing`
(регрессия на пойманный баг выше),
`a_subject_dynamic_pseudo_class_still_disables_sharing_behind_a_combinator` —
9/9 зелёных, включая три из среза 1.

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib share_cache` 9/9; `scripts/scoped-test.sh`
чист (один флак `lumen-js::frame_bridge::inaccessible_bridge_mutation_
does_not_mark_dirty` — общий процесс-глобал `take_frame_dom_dirty`,
зелёный при изолированном перезапуске, не связан с этой правкой);
`python graphic_tests/dump_golden.py --build` — 12/12.

**Что НЕ сделано** (следующий срез): `share_insert > 0` на реальных
сайтах ещё не достигнут. Два пути вперёд: (a) расширить `ShareKey`
позицией среди соседей (first/last/only-child индекс) — сделает
`:first-child`-класс pseudo-classes безопасными по тому же принципу, что
`Attribute` в этом срезе, но это отдельный, не факт что дешёвый кусок
работы; (b) сузить `RuleIndex::universal` bucket так, чтобы кандидатная
выборка не возвращала заведомо неприменимые для узла правила (общая
проблема индекса, не специфичная для `ShareCache`) — архитектурно больше
это среза. [BUG-935](BUG-935-OPEN.md)'s `LUMEN_BUG935_M4_SWAP` живой A/B
переизмерять пока рано — блокер не снят.

## Срез 4 (2026-09-24)

Путь (a) реализован — но не в исходной форме. Первая версия (добавить
`is_first_child`/`is_last_child` в `ShareKey` БЕЗУСЛОВНО, для каждого узла)
провалила `repeated_svg_icons_share_the_cascade_without_changing_the_result`
(тест среза 2, стиль-лист без единого position-селектора) — регрессия
поймана до пуша: 6 повторных иконок под одним родителем в норме различаются
только позицией среди 6 сиблингов (ровно один first, ровно один last), так
что безусловное добавление этих двух полей в ключ разбивает ЛЮБОЙ повтор
одинаковых сиблингов на минимум 3 группы вместо одной — то есть режет
именно ту выгоду (391 повторяющихся иконок → 1 аллокация), ради которой
`ShareCache` вообще существует, на КАЖДОМ сайте, даже там, где ни одно
правило вообще не читает position pseudo-classes.

Исправлено гейтом: новая `sheet_has_position_dependent_subject`
(`share_cache.rs`) один раз за проход сканирует `sheet.rules` +
`layers`/`media_rules`/`supports_rules` на предмет `:first-child`/
`:last-child`/`:only-child` в SUBJECT-позиции ЛЮБОГО селектора; `ShareKey`
получает реальные `is_first_child`/`is_last_child` только когда флаг
`true`, иначе оба поля константно `false` для всех узлов (не влияет на
партиционирование ключа вообще). Результат сохраняет и старый тест
(флаг `false` на его стиль-листе — 0 стоимости), и новый (флаг `true` —
4 из 6 сиблингов делят один ключ, ровно first/last — нет).

`selector_is_share_safe` переписана как `complex_is_share_safe`/
`compound_is_share_safe` с явным параметром `describes_key_node` вместо
булева `is_subject`, чтобы корректно рекурсировать в
`:where(..)`/`:is(..)`/`:not(..)` — живая инструментация на github.com
(временный `LUMEN_BUG1112_DEBUG=1`, не закоммичен) нашла это доминирующим
оставшимся блокером ПОСЛЕ фикса position pseudo-classes: Primer (дизайн-
система GitHub) компилирует практически каждый компонентный класс в обёртку
`:where(.prc-X-Y-Z)` для нулевой specificity (`:where(.prc-Link-Link-9ZwDx)
:where([data-muted=true]):hover` и т.п.) — раньше `PseudoClass::Where`
безусловно попадал в `_ => false`. Поскольку `:where`/`:is`/`:not`
матчат ТОТ ЖЕ узел, что и compound, в котором лежат (CSS Selectors L4
§5.4/§17), они share-safe ровно когда safe каждый селектор их
списка-аргумента — рекурсия с ТЕМ ЖЕ `describes_key_node`, не всегда
`true` (иначе `:where(:first-child)` в ANCESTOR-позиции ложно считался бы
safe).

Второй найденный блокер, после фикса `:where`/`:is`/`:not`: голый
`PseudoElement` (`::placeholder`, `::-webkit-calendar-picker-indicator` и
т.п.) без type/class/id в subject — `RuleIndex`'s `universal`-bucket отдаёт
его кандидатом для ЛЮБОГО узла. `matches_simple` (`matching.rs:223`)
безусловно возвращает `false` для ЛЮБОГО `PseudoElement` (кроме `Slotted`,
матчащегося отдельной функцией `matches_slotted_complex`, которая вызывается
только когда `host_shadow.is_some()` — уже гасит `shareable` независимо)
в обычном пути `matches_complex`, которым идёт весь `sheet.rules`/
`layers`/`media`/`supports`. Значит компаунд с `PseudoElement` НИКОГДА не
матчит ни один настоящий DOM-узел этим путём — жёсткий, безусловный
non-match, как несовпадение по `Type`, не завязанный на что-либо, чего не
ловит ключ. Добавлено: `PseudoElement(_) => true` безусловно (без гейта на
`describes_key_node` — safe и в subject, и в ancestor позиции).

**Результат живого перемера (github.com/lenta.ru) после всех трёх фиксов
среза 4: `share_insert` ОСТАЁТСЯ 0.** `key_some_unshareable` тоже не
изменилось (391 / 79) — все три фикса убрали классы блокеров, которые
покрывали ВЕСЬ candidate-набор через `universal`-bucket (или через
`:where`/`:is`/`:not`-обёртку над таким же универсальным правилом), но
`key_some_unshareable` не падал ни разу, потому что на его месте
обнаружился ЧЕТВЁРТЫЙ, архитектурно иной источник того же эффекта — уже не
в `universal`-bucket. `RuleIndex::by_class["octicon"]` (не только
`universal`) сам по себе отдаёт как кандидатов ~20 разных `.btn:hover
.octicon`/`.select-menu-item:focus .octicon`/`[aria-selected=true]
.octicon`-подобных правил для ЛЮБОГО узла с классом `octicon`, независимо
от того, является ли этот конкретный узел действительно потомком
`.btn`/`.select-menu-item`/… — потому что `candidates()` бакетирует по
СОБСТВЕННОМУ классу узла, а не по факту совпадения предка. Эти правила
корректно (не баг) остаются unsafe — они честно зависят от `:hover`/
`:disabled`/`:focus`/ancestor `aria-selected`, чего ключ не пинит и не
может пинить дёшево. Но поскольку КАЖДЫЙ `.octicon`-узел документа получает
ВСЕ ~20 таких правил кандидатами (не только те, что реально до него
дотягиваются), почти наверняка хотя бы одно из них остаётся unsafe для
каждого узла — воспроизводимо на обоих измеренных сайтах, число
`key_some_unshareable` не сдвинулось ни на единицу ни на github.com, ни на
lenta.ru после этого среза.

Это ровно путь (b) из постановки задачи выше ("сузить `RuleIndex::universal`
bucket"), только обнаружилось, что проблема шире — она не ограничена
`universal`-bucket, она есть у ЛЮБОГО бакета (`by_class`/`by_type`/`by_id`/
`by_attr`), потому что бакетирование в принципе идёт только по собственным
атрибутам узла, без учёта предков. Точечный фикс тут невозможен: нужна
либо (b1) дешёвая проверка реальной досягаемости кандидата (пройти по
предкам узла до тех пор, пока либо не найдётся совпадение, либо не
кончится SVG-presentational-сегмент — то есть частичный `matches_complex`
без последнего compound), либо (b2) смена архитектуры кэша так, чтобы
`shareable` считался не "ни одного unsafe-кандидата вообще", а per-rule, с
кэшированием per-rule результата "не дотягивается до этого узла" отдельно
от per-node ключа. Обе — не мелкая правка, следующий срез должен выбрать
между ними или найти третий путь.

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib style::tests` — 1344/1344; `scripts/
scoped-test.sh` (затронутые + обратные зависимости, включая `lumen-driver`,
`lumen-js`) — зелёный, без единого провала; `python graphic_tests/
dump_golden.py --build` — 12/12. Новые тесты (`share_cache.rs`):
`a_subject_first_child_pseudo_class_does_not_disable_sharing_when_irrelevant`,
`a_subject_first_child_pseudo_class_still_applies_correctly_when_relevant`
— 11/11 в файле.

## Срез 5 (2026-09-24)

Взят путь (a) в его узкой форме — не полная `ShareKey`-позиция среди
соседей (это уже сделал срез 4 для `:first-child`/`:last-child`/
`:only-child`), а тот же класс индукции для АТРИБУТНОГО селектора в
ancestor-позиции: `compound_is_share_safe`'s `Attribute`-ветка была
`describes_key_node` (subject-only, срез 3); теперь безусловная `true`.
Обоснование — не новая индукция, та же самая, что уже принята для
`Type`/`Class`/`Id`/`Universal` на позиции предка (`complex_is_share_safe`'s
доккомент): два узла с совпавшим ключом имеют, по этой индукции,
попарно тег+атрибуты-идентичную цепочку предков до общего живого предка
или до цепочки cache-hit-предков с тем же свойством; атрибутный селектор
на предке не может различить эти два узла по той же причине, по которой
не может `Class`/`Id`. `share_cache.rs`'s доккомент обновлён синхронно.
Регрессионный тест среза 3
(`an_ancestor_position_attribute_selector_still_disables_sharing`) не
затронут напрямую — он не эксплуатирует индукцию (два разных `<div>`,
`inherited_ptr` не совпадает), это отмечено в его теле явным комментарием;
позитивный случай, который срез 5 действительно открывает, покрыт новым
`an_ancestor_position_attribute_selector_now_shares_under_a_literal_common_parent`
(шесть SVG-иконок — буквальные дети одного `<div data-theme="b">`).

**Не закрывает `share_insert=0` из среза 4.** Четвёртый блокер — кандидаты
вида `.btn:hover .octicon`/`.select-menu-item:focus .octicon` — это
ancestor PSEUDO-CLASS-селекторы, не atribute-селекторы; они остаются
законно unsafe (динамическое состояние, ключ его не пинит ни для какого
узла). Срез 5 закрывает только подмножество четвёртого блокера, где
ancestor-компаунд — атрибутный (`[aria-selected=true] .octicon` и
однотипные); github.com/lenta.ru's доминирующие кандидаты — pseudo-class,
не attribute, так что живой `share_insert` на этих двух сайтах, по всей
видимости, останется 0 и после этого среза (не перепроверено живым
прогоном — предыдущий срез уже установил, что для `key_some_unshareable`
достаточно ОДНОГО unsafe-кандидата из ~20, и в срезе 5 не тронут состав
кандидатов, только их индивидуальная безопасность). Следующий срез должен
взять путь (b1)/(b2) из среза 3 напрямую — реальную досягаемость
кандидата, не индукцию по типу селектора — иначе прогресс к
`share_insert > 0` на реальных сайтах не гарантирован никаким дальнейшим
расширением списка "безопасных" типов ancestor-селектора.

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib style::tests::share_cache` — 12/12;
`cargo test -p lumen-layout --lib style::` — 1368/1368; `scripts/
scoped-test.sh` (`cascade.rs`/`share_cache.rs`/`tests/share_cache.rs`) —
в процессе на момент записи, дополню при завершении. Новый тест
(`share_cache.rs`):
`an_ancestor_position_attribute_selector_now_shares_under_a_literal_common_parent`.

## Срез 6 (2026-09-24)

Взят путь (b1) из среза 3/4: дешёвая проверка реальной досягаемости
кандидата вместо расширения списка "абстрактно безопасных" типов
селектора — предыдущий путь (срез 5) закрывает только атрибутные
селекторы на предке, но доминирующий блокер github.com/lenta.ru
(`.btn:hover .octicon`/`.select-menu-item:focus .octicon`-подобные
правила) — это ancestor PSEUDO-CLASS-селекторы, которые срез 5 прямо
назвал незакрытыми.

`selector_is_share_safe` получил `doc`/`node` (была чисто абстрактной
функцией селектора, теперь знает про реальный документ) и, при провале
абстрактной проверки, пробует `ancestor_prefix_could_rescue`: если
СУБЪЕКТ безопасен и все комбинаторы — `Descendant`/`Child` (иначе не
спасти — см. доккомент), проверяется, действительно ли ancestor-часть
селектора ДОСТИЖИМА от `node`'s реальных предков
(`ancestor_chain_reachable`, зеркалит back-tracking `matches_chain`, но
тестирует каждого кандидата-предка [`compound_reachable`] вместо
`matches_compound`). Если недостижима — правило физически не может
повлиять на каскад `node` НИ ПРИ КАКОМ динамическом состоянии, значит не
может угрожать и шарингу; статус меняется на safe.

`compound_reachable` — не полный `matches_compound`: у `node`'s предка
`p`, если `p` сам `is_svg_presentational_element`-пригоден, любая часть
компаунда, не входящая в уже доверенный список
(`Type`/`Class`/`Id`/`Universal`/`Attribute`), матчится ПЕРМИССИВНО
(`true`, "может совпасть", не "совпадает") — потому что индукция
`selector_is_share_safe`'s доккомента доказывает только
`tag`+`attrs`-идентичность пригодных предков между двумя
коллизирующими узлами, не идентичность СОСТОЯНИЯ: `:hover` на другом
физическом `.btn` может отличаться. С первого НЕпригодного предка
проверка становится РЕАЛЬНОЙ (обычный `matches_simple`) — это надёжно
именно потому, что непригодный узел никогда не попадает в
`ShareCache` (`build_key` возвращает `None`), так что коллизия
`ShareKey.inherited_ptr` НА ЭТОМ уровне возможна только если это
буквально один и тот же живой узел для обоих коллизирующих экземпляров
(см. `share_cache.rs`'s доккомент) — а значит реальное состояние там
одинаково для обоих по построению, не по доказательству.

**Найденный до коммита баг в собственном фиксе** (тем же классом
ошибок, что уже ловили срезы 3-4): первая версия `ancestor_prefix_
could_rescue` возвращала `ancestor_chain_reachable(..)` НАПРЯМУЮ вместо
`!ancestor_chain_reachable(..)` — инвертированная логика: "предок
реально существует" ошибочно читалось как "можно простить", а
"предка нет" — как "не прощать". Новый тест `a_reachable_ancestor_
hover_pseudo_class_still_disables_sharing` поймал это немедленно (оба
`<g class="wrap">`-обёрнутых значка получали цвет ОТ `:hover`, хотя
наведён был только первый — утечка через ошибочно расшаренный кэш);
симметрично, `an_unreachable_ancestor_hover_pseudo_class_does_not_
disable_sharing` падал в другую сторону (шаринг не включался там, где
должен был). Оба теста зелёные после инверсии.

Новые тесты (`share_cache.rs`):
`an_unreachable_ancestor_hover_pseudo_class_does_not_disable_sharing`
(доминирующий реальный случай — `.btn:hover .octicon`, `.btn` в
документе нет вообще, шаринг теперь включается) и
`a_reachable_ancestor_hover_pseudo_class_still_disables_sharing`
(регрессионный якорь на инверсию выше — два структурно идентичных
`<g class="wrap">` под одним `<svg>`, что даёт им коллизию по ключу,
наведён только первый, второй не должен получить его `:hover`-цвет).

**Живой перемер (github.com/lenta.ru) в этой сессии не проведён** — нет
сетевого доступа в песочнице (тот же блокер, что и в предыдущих срезах).
По коду ожидание: `.btn:hover .octicon`/`.select-menu-item:focus
.octicon` (github.com) остаются UNSAFE, если реальный `.btn`/`.select-
menu-item`-предок действительно существует над КОНКРЕТНЫМ значком (что
для меню/кнопок, реально содержащих иконку, как правило так и есть) —
этот срез не закрывает "иконка внутри настоящего `.btn`", только
"иконка, для которой `RuleIndex`'s бакетирование по собственному классу
подсунуло кандидата, никогда физически не достижимого от неё". Следующий
срез должен переизмерить `share_insert` живым прогоном, чтобы понять,
какая доля 391/79 `key_some_unshareable`-узлов реально не имеет
`.btn`/`.select-menu-item`-предка (первый путь, теперь закрыт) против
тех, что имеют (второй путь, всё ещё требует полноценного (b2) —
per-rule reachability caching, а не одной проверки на кандидата).

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib style::tests::share_cache` — 14/14;
`cargo test -p lumen-layout --lib style::` — 1370/1370; `scripts/
scoped-test.sh crates/engine/layout/src/style/cascade.rs
crates/engine/layout/src/style/tests/share_cache.rs` — зелёный кроме
уже задокументированного флака `lumen-js::frame_bridge::
inaccessible_bridge_mutation_does_not_mark_dirty` (см. срез 3 —
процесс-глобал `take_frame_dom_dirty`, зелёный при изолированном
перезапуске; подтверждено повторно в этой сессии); `python
graphic_tests/dump_golden.py --build` — 12/12.

## Срез 7 (2026-09-24)

Живой перемер `share_insert`, которого просил срез 6 — сеть в этой
сессии оказалась доступна (`curl https://github.com` → 200), в отличие
от большинства предыдущих срезов. Без изменений кода, только измерение
`LUMEN_SHARECACHE_STATS=1 ./target/dev-release/lumen.exe --maximized
<url>` (бинарник собран на коде среза 6, уже в `main`).

**github.com:** `hit=0 insert=0 miss=1784 (key_none=1393
key_some_unshareable=391)` — число `key_some_unshareable` БУКВАЛЬНО не
изменилось относительно замера ДО среза 6 (391, та же цифра, что
фигурирует в срезах 3-5). **lenta.ru:** `hit=0 insert=0 miss=1510
(key_none=1427 key_some_unshareable=83)` — было 79-81 в разных прошлых
замерах (обычный дрейф контента страницы/рекламы между сессиями, не
регрессия); `insert` тоже 0.

**Вывод подтверждает прогноз среза 6 буквально, а не приблизительно:**
на обоих сайтах доминирующий случай — иконка внутри РЕАЛЬНО
существующего `.btn`/`.select-menu-item`-предка, то есть путь (b1)
среза 6 (дешёвая reachability-проверка кандидата) не мог и не должен
был снизить `key_some_unshareable` на этих конкретных страницах: он
закрывал только случай «кандидат недостижим», которого здесь, судя по
неизменному числу, нет вовсе или пренебрежимо мало. Открытый вопрос
среза 6 («какая доля 391/79 реально не имеет предка против тех, что
имеют») закрыт: на github.com/lenta.ru — доля первой категории
неотличима от нуля.

Это исчерпывает путь (b1) как источник дальнейшего прогресса на этих
двух сайтах. Единственный оставшийся путь вперёд для `.btn:hover
.octicon`-класса селекторов — путь (b2), явно отложенный срезом 6:
per-rule/per-node кэширование факта «состояние ИМЕННО ЭТОГО предка
(`:hover`/`:focus`/…) участвует в стиле узла», то есть shareability,
завязанная на состояние предка, а не только на его существование —
качественно другая, более дорогая структура (не просто предикат над
`ShareKey`, а что-то вроде отдельного invalidation-набора на предка),
не однодневный срез. До неё `share_insert` на github.com/lenta.ru
останется 0 для узлов этого класса; страницы без `:hover`/`:focus`-
зависимых предковых правил (см. срез 2's синтетический тест) от этого
ограничения не страдают.

**Не сделано:** путь (b2) не спроектирован и не начат — стоит отдельным
следующим срезом или отдельным решением, продолжать ли вообще (текущая
цена — 391/83 несброшенных SVG-узлов на двух конкретных сайтах, а не
провал всего механизма: `ShareCache` по-прежнему корректен, просто
консервативен для этого одного класса правил). [BUG-935](BUG-935-OPEN.md)'s
`LUMEN_BUG935_M4_SWAP` живой A/B по-прежнему рано переизмерять этим —
блокер (нулевой `cascade_reused`) этим срезом не снят.

Гейты: без изменений кода в этом срезе — `cargo clippy -p lumen-layout
--all-targets -- -D warnings` чист (неизменный код).

## Срез 8 (2026-09-24)

Первая реализация пути (b2) — per-rule фингерпринт вместо структурного
запрета. Ключевое переосмысление: `ShareCache` пересоздаётся с нуля на
КАЖДЫЙ проход каскада (см. module doc `share_cache.rs`) — она никогда не
переживает изменение динамического состояния между проходами. Значит не
нужна отдельная invalidation-структура на состояние предка, которую
предполагал срез 7 — достаточно включить в `ShareKey` РЕАЛЬНЫЙ, текущий
результат `matches_complex` для проблемного селектора: два узла делят кэш
только когда этот бит у них тоже совпадает, то есть их фактическое
состояние (наведён ли `.btn`/`.select-menu-item` сейчас) идентично — ровно
то условие, при котором шаринг действительно безопасен, не приближение к
нему.

Изменено:
- `selector_is_share_safe` (`cascade.rs`) больше не зовёт `doc`/`node`
  вообще — стал чисто абстрактной функцией: `complex_is_share_safe(sel,
  true) || ancestor_prefix_is_fingerprintable(sel)`.
- `ancestor_prefix_could_rescue`/`ancestor_chain_reachable`/
  `compound_reachable` (срез 6) удалены — их работу теперь делает
  комбинация `ancestor_prefix_is_fingerprintable` (тот же формообразующий
  тест: субъект безопасен, все комбинаторы `Descendant`/`Child`, БЕЗ
  реального обхода предков) и новая `dynamic_ancestor_fingerprint`.
- `dynamic_ancestor_fingerprint(doc, node, sheet, viewport, dark_mode)` —
  проходит те же четыре источника кандидатов (`rules`/`@layer`/`@media`/
  `@supports`), что и `compute_style_shareable`, и для каждого селектора,
  который абстрактно небезопасен, но фингерпринтуем, зовёт РЕАЛЬНЫЙ
  `matches_complex(sel, doc, node)` — тот же матчер, каким кэш уже
  доверяет реальному применению правил, не отдельная permissive-версия
  среза 6. Результат — `Vec<bool>`, один бит на такой селектор, в порядке
  `RuleIndex::candidates`, который детерминирован по tag/id/classes/attrs
  узла — тем самым уже пинуется ключом, так что у двух коллизирующих по
  ключу узлов вектор гарантированно той же длины и того же порядка.
- `ShareKey` получил поле `dynamic_ancestor_sig: Vec<bool>`, `build_key`
  считает его только когда новый sheet-wide гейт
  `sheet_has_fingerprintable_ancestor_selector` (по образцу среза 4's
  `sheet_has_position_dependent_subject`) нашёл хоть один такой селектор в
  листе — иначе `Vec::new()` без обхода `RuleIndex` вообще, на любом сайте
  без правил вида `.btn:hover .foo`.

Попутное упрощение: механизм срезов 6-7 (структурное доказательство
недостижимости) стал частным случаем нового — недостижимый предок просто
даёт `matches_complex == false` детерминированно для любого узла без
такого предка, что и раньше признавалось безопасным, но теперь без
отдельной permissive/real развилки в `compound_reachable`.

Новый тест `agreeing_reachable_ancestor_hover_pseudo_class_state_now_shares`
— три структурно идентичных `<g class="wrap">`, НИ ОДИН не наведён,
`.wrap:hover .octicon` в листе: три иконки делят один `Arc`, ровно случай,
который срез 6/7 не мог закрыть (реальный `.btn`-предок физически
существует, поэтому старая reachability-проверка навсегда объявляла
результат unsafe независимо от фактического состояния наведения).
Регрессионный `a_reachable_ancestor_hover_pseudo_class_still_disables_sharing`
(один `<g>` наведён, другой нет — разные биты фингерпринта, шаринг не
включается, утечки `:hover`-цвета нет) остаётся зелёным без изменений — это
именно то различие, ради которого поле вообще существует. Все 14
существовавших тестов `share_cache` зелёные без изменений поведения.

**Живой перемер (github.com/lenta.ru) в этой сессии не проведён** —
следующий срез должен подтвердить `share_insert > 0` тем же
`LUMEN_SHARECACHE_STATS=1` прогоном, которым срез 7 замерил 391/83. По
коду ожидание: страницы, где для конкретного `.octicon`-узла ни один из
~20 `:hover`/`:focus`-кандидатов не наведён В ЭТОТ МОМЕНТ (подавляющее
большинство узлов документа в любой конкретный момент — наведён максимум
один элемент интерфейса), теперь получают одинаковый (все `false`)
`dynamic_ancestor_sig` и должны начать шариться; `share_insert` должен
стать заметно больше 0. [BUG-935](BUG-935-OPEN.md)'s
`LUMEN_BUG935_M4_SWAP` живой A/B стоит переизмерить только после этого
подтверждения, не раньше.

**Не сделано:** живой перемер `share_insert`; переизмерение
`LUMEN_BUG935_M4_SWAP` на lenta.ru; аудит, не создаёт ли фингерпринт
дополнительную стоимость на очень больших `RuleIndex`-бакетах (не
измерено — только 14 юнит-тестов и клиппи в этом срезе, сеть в этой
сессии не проверялась заранее).

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib style::tests::share_cache` — 15/15;
`cargo test -p lumen-layout --lib style::` — 1371/1371; `scripts/
scoped-test.sh crates/engine/layout/src/style/cascade.rs
crates/engine/layout/src/style/share_cache.rs
crates/engine/layout/src/style/tests/share_cache.rs
crates/engine/layout/src/style/share_safety.rs` — зелёный (exit 0).

## Срез 9 (2026-09-24)

Живой перемер, которого просил срез 8 — сеть снова доступна в этой
сессии. `LUMEN_SHARECACHE_STATS=1 LUMEN_NO_ADBLOCK=1
./target/dev-release/lumen.exe --screenshot <out.png> <url>` (бинарник
собран на коде среза 8, ещё не в `main`):

- **github.com:** `hit=0 insert=0 miss=1780 (key_none=1389
  key_some_unshareable=391)`.
- **lenta.ru:** `hit=0 insert=0 miss=1498 (key_none=1415
  key_some_unshareable=83)`.

Оба числа **бит-в-бит совпадают** с базой среза 7 (391/83, замер ДО
среза 8). Срез 8 не изменил классификацию НИ ОДНОГО кандидата на этих
двух сайтах — гипотеза среза 8 («страницы, где ни один `:hover`-кандидат
не наведён, начнут шариться») не подтвердилась, по коду понятно почему:

Доминирующий блокер на github.com (комментарий `compound_is_share_safe`,
срез 4, разбирающий Primer) — `:where(.prc-Link-Link-9ZwDx):where([data-muted=true]):hover`
и однотипные. `:hover` здесь стоит НЕ на анцестор-компаунде через
комбинатор, а на САМОМ SUBJECT-компаунде (единственный компаунд,
`sel.tail` пуст). `ancestor_prefix_is_fingerprintable` (`share_safety.rs:134-139`)
явно возвращает `false`, когда `n == 1` (нет анцестор-компаунда вообще) —
до какой-либо проверки самого `:hover`. Это не упущение реализации, а
осознанная граница дизайна среза 8 (см. doc comment `ancestor_prefix_is_
fingerprintable`: «a dynamic subject part... nothing ancestor-shaped to
fingerprint»), но она означает, что срез 8 архитектурно не может
задеть ИМЕННО тот паттерн, который сам же и определил доминирующим на
github.com. Механизм срезов 6-8 (ancestor fingerprint) решает другой
класс блокеров (`.wrap:hover .octicon` — `:hover` на предке, до которого
есть комбинатор) — реальный на других сайтах/сценариях (тест
`agreeing_reachable_ancestor_hover_pseudo_class_state_now_shares`
демонстрирует его корректно), но не на измеренных github.com/lenta.ru,
где counted-блокер (391/83) — subject-level.

**Не проверено (следующий срез, если он появится):** совпадает ли
`key_some_unshareable=83` на lenta.ru с тем же subject-`:hover`-паттерном
или с чем-то ещё — не разбирался построчно, только числовое совпадение
с базой. Для реального прогресса на github.com нужен отдельный
механизм — фингерпринт SUBJECT-компаунда самого узла (не предка): для
`node:hover` контракт «безопасно шарить, когда РЕАЛЬНОЕ `:hover`-состояние
узла совпадает» тот же самый, что уже реализован для предков, просто на
глубине 0 вместо >0 — `ShareKey` уже пинует часть собственных
per-node фактов узла (`is_first_child`/`is_last_child`), это тот же
класс расширения. Не сделано в этом срезе — отдельная задача.

`LUMEN_BUG935_M4_SWAP` живой A/B на lenta.ru НЕ переизмерен: срез 8
не сдвинул `share_insert` (остался 0), так что предпосылка среза 7
(«переизмерить после share_insert > 0») не выполнена — переизмерение
осталось бы тем же экспериментом на неизменном состоянии.

## Срез 10 (2026-09-24)

Реализован механизм, который срез 9 назвал недостающим: фингерпринт
самого SUBJECT-компаунда узла (не только предка). `ancestor_prefix_
is_fingerprintable`/`dynamic_ancestor_fingerprint`/`ShareKey::dynamic_
ancestor_sig` переименованы и обобщены (`is_fingerprintable`/
`dynamic_fingerprint`/`ShareKey::dynamic_fingerprint_sig`) как объединение
двух форм — анцестор-форма (срез 8) и новая `subject_is_fingerprintable`:
эскейп для селектора вида `node:hover` — все компаунды-предки (если
есть) должны быть структурно safe, сам subject-компаунд не проверяется
(его реальный матч и так фолдится в ключ через `matches_complex`).
Два новых теста-регрессии (`a_subject_hover_pseudo_class_still_disables_
sharing_when_state_disagrees`, `agreeing_subject_hover_pseudo_class_
state_now_shares`) воспроизводят ровно найденный в срезе 9 паттерн
(`:where(...):hover` без анцестор-компаунда) и проходят.

Живой перемер (`LUMEN_SHARECACHE_STATS=1 LUMEN_NO_ADBLOCK=1
./target/dev-release/lumen.exe --screenshot <out> <url>`, бинарник
собран на коде этого среза) — **числа бит-в-бит совпадают со срезом 9**:

- **github.com:** `insert=0 miss=1780 (key_none=1389 key_some_unshareable=391)`.
- **lenta.ru:** `insert=0 miss=1498 (key_none=1415 key_some_unshareable=83)`.

Срез 10 не задел НИ ОДНОГО кандидата на обоих сайтах — притом что
механизм реализован корректно и тесты его подтверждают. Причина:
**срез 9 неверно диагностировал доминирующий блокер по чтению кода**,
не по прямому замеру. Временный зонд (`eprintln!` в `compute_
style_shareable` на каждом небезопасном `rule.selector_text()` для
SVG-presentational узлов — не закоммичен, откачен после измерения)
показал реальную картину:

- **github.com:** доминируют селекторы с sibling-комбинатором, subject
  которых `*`/`:not(label)` (Primer, преимущественно внутри `@layer`) —
  `.prc-FormControl-ControlVerticalLayout-8YotI > :not(label) + *`
  (23112 срабатываний зонда), `... h1 + *, ... h2 + *, ... p` (23112),
  `.prc-CheckboxOrRadioGroup-Body-S3dlj > * + *` (11556),
  `...:hover ... path:nth-of-type(2), ...:focus-within ...` (11064),
  `... * + *` (7704×2), плюс не-Primer `.PhoneInputWithCountrySearch-…
  > :first-child > :nth-child(2)` (1926) и `.radio-input:disabled +
  .radio-label .octicon` (165). Ни одного случая доминирующего
  `:where(...):hover`-паттерна срез 9 не нашёл в зонде вообще — то
  есть либо он не входит в кандидаты именно для SVG-presentational
  узлов (RuleIndex-бакетирование по классу, а не universal, как решил
  срез 9 по чтению исходника), либо он действительно есть, но полностью
  замаскирован количеством sibling-блокеров, которые сами по себе уже
  дисквалифицируют те же самые 391 узла.
- **lenta.ru:** один-единственный блокер объясняет все 83:
  `.goodnews__label:hover :is(.goodnews__checkbox:checked ~
  .goodnews__tooltip._turn-off, .goodnews__checkbox:not(:checked) ~
  .goodnews__tooltip._turn-on)` (360 срабатываний зонда) — sibling-
  комбинатор `~` СПРЯТАН внутри аргумента `:is(...)`, что делает саму
  `:is(...)`-компаунду структурно unsafe (`complex_is_share_safe`
  рекурсивно требует `Descendant`/`Child` и для внутренних комбинаторов)
  независимо от того, что снаружи неё стоит фингерпринтируемый
  `:hover`-предок.

Оба случая — селекторы с sibling-комбинатором (`+`/`~`), для которых
`ShareKey` архитектурно не имеет и не может дёшево завести поле: в
отличие от `is_first_child`/`is_last_child` (позиция ДАННОГО узла среди
СВОИХ соседей — O(1) факт), общий sibling-комбинатор требует знать
identity/состояние ПРОИЗВОЛЬНОГО количества соседей на произвольную
глубину (`~` — "любой предшествующий", не только соседний), что не
сводится к конечному набору bool-полей ключа тем же способом. Фингерпринт
среза 8/10 в принципе не может рескьюить эти селекторы, потому что
`ancestor_prefix_is_fingerprintable`/`subject_is_fingerprintable` обе
требуют `Descendant`/`Child`-комбинаторы структурно — sibling-комбинатор
отклоняется до попытки фингерпринта, по замыслу (см. doc comment
`selector_is_share_safe`).

**Вывод:** срез 10 — корректное, протестированное обобщение механизма
(полезно как факт для будущих сайтов с чистым `node:hover`-паттерном без
sibling-соседей), но НЕ прогресс по измеренной цели (`share_insert` на
github.com/lenta.ru). Реальный блокер на этих двух сайтах — sibling-
комбинаторы, путь вперёд для них лежит не через фингерпринт, а либо
через (a) добавление в `ShareKey` ограниченной формы sibling-identity
(например, только `:nth-child`/`:nth-of-type` с константным индексом —
тот же класс, что уже решён для `first`/`last`, но существенно шире:
`nth-of-type(N)` для произвольного N, не только 1), либо (b) сужение
`RuleIndex`-кандидатуры так, чтобы sibling-комбинаторные правила не
попадали в кандидаты SVG-presentational узлов, когда их subject-класс
заведомо не соответствует (не применимо здесь — `*`/`:not(label)` как
subject делает кандидатуру для любого узла законной, не багом
бакетирования), либо (c) признать эту ветку BUG-1112 исчерпанной для
данных двух сайтов и закрыть как «известное архитектурное ограничение»
без дальнейших срезов в этом направлении.

Гейты: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист;
`cargo test -p lumen-layout --lib style::tests::share_cache` — 17/17;
`cargo test -p lumen-layout --lib style::` — зелёный.
