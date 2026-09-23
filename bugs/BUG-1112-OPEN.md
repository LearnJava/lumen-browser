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
