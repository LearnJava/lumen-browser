# BUG-950 — `animation-timeline: scroll()` не двигает вычисленный стиль анимации: Phase 1 остаётся незаведённой

**Статус:** OPEN (ДОРАБОТКА → [ROADMAP.md](../ROADMAP.md), задача `P2-scrolldriven`)
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

## Направление починки

Это уже задача `P2-scrolldriven` в `ROADMAP.md` (заметка обновлена этой же
записью), не новый баг с нуля: Phase 0 (JS-объектная модель `ScrollTimeline`/
`ViewTimeline`, `currentTime`) готова; Phase 1 требует спроектировать путь
`ScrollTimeline.currentTime` → `AnimationScheduler` → пересчёт keyframe-
прогресса для элементов с `animation-timeline: scroll(...)`/`view(...)`,
симметрично тому, как обычные `@keyframes`/`transition` уже считают
прогресс по времени.
