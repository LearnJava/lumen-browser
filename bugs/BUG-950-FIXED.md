# BUG-950 — `animation-timeline: scroll()` не двигает вычисленный стиль анимации: Phase 1 остаётся незаведённой

**Статус:** FIXED 2026-09-19 (P1)
**Тип:** нереализованная функциональность — собственный doc-комментарий модуля прямо называет это Phase 1, невыполненной: «Phase 1 (P4): `animation-timeline: scroll()` parsed in ComputedStyle + wired to `AnimationScheduler` to drive keyframe progress from `currentTime`» (`crates/js/src/scroll_timeline.rs:16`). Объём — не точечная правка: нужна проводка от таймлайна к планировщику анимаций для целого класса keyframe-свойств, а не один член.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31, живая проба `verify_scroll_view_transition_gaps.py --variant scroll-timeline-elementsfrompoint`)
**Область:** js (`crates/js/src/scroll_timeline.rs` — JS-объектная модель, готова, Phase 0), layout (проводка в `AnimationScheduler`, ещё не сделана, Phase 1)
**Владелец:** P2 (существующая задача `P2-scrolldriven`, `ROADMAP.md`).

## Симптом

`#scroller { animation-name: anim; animation-duration: 10s;
animation-timeline: scroll(self); }` со скроллом внутри контейнера не меняет
`getComputedStyle(scroller).backgroundColor` вообще — значение остаётся
исходным до и после прокрутки, хотя `CSS.supports('animation-timeline:
scroll()')` — `true` и свойство парсится (не строка ошибки cascade).

Не путать с [BUG-231](BUG-231-FIXED.md) (FIXED) — та чинила композит-путь
(*рендер* уже вычисленного override цвета в живом окне без relayout); здесь
речь о более раннем шаге — сам *вычисленный стиль* никогда не продвигается,
поэтому композитить попросту нечего.

## Прямое измерение

Живая проба (`--variant scroll-timeline-elementsfrompoint`, dev-release):
`bg-before-efp = rgba(0, 0, 0, 0)`, скролл контейнера на 200px, `bg-after-
efp = rgba(0, 0, 0, 0)` — идентично. `elementsFromPoint` (побочный предмет
того же варианта) отрабатывает штатно (`typeof === 'function'`, вызывается
без исключения) — второй вопрос варианта закрыт, дефекта там нет.

## Кого это держит

`scroll-animations/scroll-timelines/scroll-timeline-snapshot-
elementsFromPoint.html` — прямых id в остатке нет; вариант служит
подтверждением родового механизма, которым объясняется целая ветка
не столько TIMEOUT, сколько FAIL/непройденных assertion в
`scroll-animations/`.

## Направление починки (устарело — см. «Исправлено» ниже)

Это уже задача `P2-scrolldriven` в `ROADMAP.md` (заметка обновлена этой же
записью), не новый баг с нуля: Phase 0 (JS-объектная модель `ScrollTimeline`/
`ViewTimeline`, `currentTime`) готова; Phase 1 требует спроектировать путь
`ScrollTimeline.currentTime` → `AnimationScheduler` → пересчёт keyframe-
прогресса для элементов с `animation-timeline: scroll(...)`/`view(...)`,
симметрично тому, как обычные `@keyframes`/`transition` уже считают
прогресс по времени.

## Исправлено (P1, 2026-09-19)

Ревизия объёма показала: сама проводка `ScrollCtx`/`progress_for` уже была
на месте с 2026-06-22 (`F2-2`), и getComputedStyle-патчинг оверрайдов уже
общий (GAP-CSSANIM срезы 2-8, 2026-09-16/17) — обе половины, которые этот
баг числил недостающими («Phase 1»), на самом деле существовали. Заявка
была стала (doc drift), но живой репро с исходной страницы бага всё ещё
воспроизводился — по другой причине.

Настоящий дефект — `crates/shell/src/animation_scheduler.rs::ScrollCtx::
progress_for`, ветка `AnimationTimeline::Scroll { axis, .. }`: поле `nearest`
(различает `scroll(root)` от `scroll(self)`/`scroll(nearest)`) деструктурировалось
и отбрасывалось, `ScrollTimeline { element: None, .. }` подставлялся всегда —
то есть `scroll(self)` молча резолвился в прогресс **корневого вьюпорта**,
а не собственного скролла контейнера. Раз страница из репро не скроллится
(скроллится только `#scroller`), корневой прогресс оставался 0 всю дорогу.

**Правка:** новая `find_nearest_scroll_container` (`crates/engine/layout/
src/scroll_timeline.rs`) строит путь от `root` до анимируемого узла и
возвращает ближайший ancestor-or-self, устанавливающий scroll-контейнер
(`overflow` ∈ {`scroll`, `auto`, `hidden`} по любой оси — та же дефиниция,
что уже использует `box_tree/predicates.rs::scrollbar_gutter_inline`);
`None`, если такого нет (fallback на корневой вьюпорт — корректно и для
`scroll(root)`, и для случая без скроллящегося предка). `ScrollCtx::
progress_for` вызывает её при `nearest == true`, оставляет `element: None`
при `nearest == false` (`scroll(root)`).

Различие `self` (спека требует, чтобы контейнером был сам элемент) и
`nearest` (ближайший предок-или-сам) внутри `nearest: bool` не разведено —
оба парсятся в `nearest: true` уже на уровне `parse_scroll_fn`
(`style/parse/timeline.rs`), и `find_nearest_scroll_container` ищет
ancestor-or-self для обоих одинаково. На практике это покрывает основной
случай (элемент с `animation-timeline: scroll(self)` сам объявляет
`overflow`, как в репро этого бага) и расходится со спекой только когда
`self` указан на элементе, который сам НЕ scroll-контейнер (спека там
требует inactive timeline, а этот код ищет предка) — отдельный, более
редкий остаток, не заведён отдельным номером.

5 новых юнит-тестов в `scroll_timeline.rs` (`find_nearest_scroll_container`:
прямой родитель, сам элемент, пропуск не-scrolling предков, fallback в
`None`, узел не найден) + 2 в `animation_scheduler.rs`
(`progress_for_scroll_self_uses_own_container_not_root`,
`progress_for_scroll_root_keyword_ignores_own_container`). `cargo clippy -p
lumen-layout -p lumen-shell --all-targets -- -D warnings` чист.

Живой A/B (ad-hoc проба, не закоммичена): страница с `#scroller { overflow:
auto; animation-timeline: scroll(self); animation-fill-mode: both; }`,
контейнер скроллится на середину диапазона, страница затем скроллится
отдельно на 2000px. До правки — `getComputedStyle(scroller).backgroundColor`
не сдвигался от скролла контейнера вовсе (оставался on `from`-цвете,
воспроизводя ровно симптом этого бага). После правки — сдвигается на
скролл контейнера (сверено с ожидаемой `ease`-интерполяцией на progress
0.5) и остаётся неизменным при последующем скролле страницы — `scroll(self)`
корректно независим от `scroll(root)`.
