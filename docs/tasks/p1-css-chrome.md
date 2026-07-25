# P1-CC: Хром браузера через собственный CSS-движок (Вариант A)

**Владелец: P1 — все срезы дорожки, включая CSS-доводку движка (этап A′): решение пользователя 2026-07-24, осознанное отступление от дефолтного разделения ролей (CSS-свойства обычно домен P4).** Дорожка в ROADMAP.md: `CC` (строки `CC-0`…`CC-17` + `CC-CSS-1`…`CC-CSS-6`, заведены 2026-07-24).
**Эталон:** [docs/design/lumen-v3_3.html](../design/lumen-v3_3.html) (идентичен `Design_all_variants/lumen-v3_3.html`, 1958 строк). В отличие от дорожки DS, где эталон — только визуальная истина, здесь его HTML/CSS **становятся исходником интерфейса**: хром рисуется тем же конвейером `html-parser → css-parser → layout → paint`, что и страницы.

**Статус: дорожка заведена в ROADMAP.md 2026-07-24 по указанию пользователя, все задачи назначены P1** (указатели — в `STATUS-P1.md`). Исполнение начинается с CC-0 (ADR + судьба DS-дорожки): дорожка заменяет ручную реализацию DS-1…DS-19 в перспективе — legacy-код хрома удаляется только после флипа дефолта (этап D).

---

## 1. Суть механизма

Сегодня хром — параллельная ручная система: `toolbar.rs`/`tabs/strip.rs`/`panels/*` строят `Vec<DisplayCommand>` руками, цвета из Rust-`Palette` ([panels/themes.rs:184](../../crates/shell/src/panels/themes.rs)), hit-test — своя координатная математика от `CHROME_H` ([toolbar.rs:39](../../crates/shell/src/toolbar.rs)). По сути это ручная трансляция CSS эталона в Rust-константы.

Целевая схема (Вариант A):

```
compile-time (build.rs крейта lumen-chrome):
  assets/chrome/chrome.html + chrome.css  ──парс-валидация──►  ошибка сборки при битом CSS
                                          ──кодогенерация──►  ids::* (типизированные NodeId),
                                                              enum ChromeAction (из data-action),
                                                              инвентарь <template>
runtime (шелл, за флагом LUMEN_CSS_CHROME):
  chrome Document + Stylesheet (распарсены 1 раз при старте)
    │  ChromeModel (вкладки, URL, профиль, тема) ──мутации──► DOM (атрибуты/текст/клоны шаблонов)
    ▼
  layout_measured_hyp(размер окна) ──► LayoutBox хрома ──► paint ──► overlay DisplayList
    │                                        │
    │                                        └─ lumen_paint::hit_test(x,y) ──► NodeId ──► ChromeAction
    └─ rect элемента #page-host ──► смещение/вьюпорт страницы (замена константы CHROME_H)
```

Композиция не меняется: `RenderBackend::render(content, overlay, …)` ([render_thread.rs:259](../../crates/shell/src/render_thread.rs)) уже принимает страницу и оверлей раздельно — движковый хром просто занимает слот overlay вместо ручного.

### Что компилируется заранее, что нет

| Стадия | Где выполняется | Примечание |
|---|---|---|
| Парсинг HTML/CSS хрома | **compile-time-валидация** + 1 раз при старте | Полная сериализация распарсенного дерева — опциональный CC-16 |
| Кодогенерация id/действий/шаблонов | **compile-time** | Строковые опечатки в обращениях к хрому = ошибка сборки |
| Матчинг селекторов + каскад | runtime, каждый рестайл | `compute_style` вплавлен в `build_box` ([box_tree.rs:3844](../../crates/engine/layout/src/box_tree.rs)), входа «готовые стили» нет; вынос матчинга в compile-time — исследовательский CC-17 |
| Layout | runtime | Зависит от размера окна, числа вкладок, текста — принципиально не предвычисляем |
| Paint | runtime, каждый кадр | |

## 2. Фактическая база (сверено с кодом 2026-07-23)

**Покрытие CSS — практически полное.** Макет использует ~87 свойств; всё критичное реализовано: flex, grid (`repeat()/minmax()/auto-fill`), `var()`, `calc()`, transitions, `@keyframes`, `transform`, `backdrop-filter`, `box-shadow`, градиенты, `::before/::after` c `attr()`, `:hover/:focus/:active/:focus-within`. Не поддержаны только нестандартные `::-webkit-scrollbar*` и `-webkit-font-smoothing` — косметически несущественно (есть стандартные `scrollbar-width/color`). Частично: `resize: vertical` (парсится, drag-UI нет — DevTools-панель не ресайзится), sticky scroll-follow.

**Динамика уже есть в движке:**
- `:hover/:focus/:active` — thread-locals через `lumen_layout::set_interactive_state`, шелл уже дёргает их для страниц ([main.rs:8283](../../crates/shell/src/main.rs)); смена hover = полный рестайл+релэйаут (style-only fast path отсутствует).
- Transitions + `@keyframes` + `@starting-style` — `TransitionScheduler`/`AnimationScheduler` ([animation.rs:778, 1148](../../crates/engine/layout/src/animation.rs)) тикаются в кадровом цикле ([main.rs:14186](../../crates/shell/src/main.rs)).
- Hit-test по layout-дереву — `lumen_paint::hit_test` ([hit_test.rs:77](../../crates/engine/paint/src/hit_test.rs)) возвращает узел, путь предков, курсор, `user_select`, с обратными трансформами.
- Инкрементальный релэйаут — `layout_mutation_incremental` ([box_tree.rs:2728](../../crates/engine/layout/src/box_tree.rs)) переиспользует геометрию неизменённых поддеревьев (каскад — полный).

**Границы конвейера:** вход — только байты/строки (`PageSource::Static` → `load_bytes` → обычный парс, [main.rs:3614](../../crates/shell/src/main.rs)); промежуточного styled-tree нет; `ComputedStyle`/`LayoutBox`/`DisplayCommand` — plain-структуры без serde.

**Состав макета:** ~700 строк CSS (~300 правил), ~350–400 DOM-элементов, ~400 строк JS (~40 интеракций — все переезжают в Rust, JS в хроме не будет). Темизация целиком на custom properties: `:root` (токены) + `body[data-theme=…]` (2 темы) + `body[data-profile=…]` (4 профиля) + `--ws-color` (workspaces) + `body[data-layout=…]` (верт./гориз. вкладки). Иконки — один инлайн SVG-спрайт ~35 `<symbol>`, картинок нет. **Не продукт** (вырезается при подготовке ассетов): demo-bar с 7 вариантами формы, QA-панель тестировщика, `[data-tip]`-тултипы спецификации, Google-Fonts `<link>` (шрифты уже бандлены с DS-4: Golos Text + JetBrains Mono).

## 3. Правила для каждого среза (обязательны)

1. Один срез = одна сессия = одна ветка `p1-cc-<N>-<slug>` в своём worktree `.claude/worktrees/<task>/`. Завершение — `/lumen-task-finish`.
2. Перед стартом: `git pull origin main`, `git branch` — проверить занятые `p1-cc-*`.
3. До CC-14 (флип) новый путь живёт **строго за флагом** `LUMEN_CSS_CHROME=1` (env) — дефолтное поведение окна не меняется ни на пиксель. Смешение разрешено: движковый хром + legacy-панели поверх (painter's order в `overlay_buf` это уже позволяет).
4. Ассеты `assets/chrome/*` — производные от замороженного эталона; правка дизайна = новая версия эталона, затем регенерация ассетов. Руками в ассетах не «подкручивать под движок»: расхождение рендера = баг движка → BUG-NNN (это dogfooding, главная побочная ценность дорожки).
5. Хром не рисуется в headless CPU-пути — CPU-снапшоты страниц срезы не трогают (исключение: если срез меняет смещение страницы — прогнать `graphic_tests/run.py` смоук).
6. Cargo только `-p`, clippy `-D warnings`, `///`-доки, doc-sync (`CAPABILITIES.md`, `subsystems/shell.md` + новый `subsystems/chrome.md` после CC-3) — в том же коммите.

---

## Этап 0 — решение

### CC-0 (S): ADR «Хром через собственный движок» + судьба дорожки DS

Написать `docs/decisions/ADR-021` (номер уточнить по индексу): контекст (ручная DS-трансляция vs dogfooding движка), решение, флаг-стратегия по образцу ADR-018 (V8-cutover: opt-in → паритет → флип дефолта → rollback-флаг → удаление legacy слайсами). Зафиксировать судьбу DS: дорожка DS-1…DS-19 завершена 2026-07-23, новых ручных DS-задач не заводить; legacy-код хрома живёт до этапа D. Строки CC в `ROADMAP.md` уже заведены 2026-07-24. **DoD:** ADR в индексе.

## Этап A — валидация гипотезы (без wiring, самый дешёвый способ убить/подтвердить идею)

### CC-1 (M): Производные ассеты + рендер-смоук через существующий движок

1. `scripts/gen_chrome_assets.py`: из эталона вырезать demo-bar, QA-панель, `[data-tip]`-правила, `<script>`, Google-Fonts-`<link>` (font-family уже указывает на бандленные Golos Text/JetBrains Mono с system-fallback); результат — `assets/chrome/chrome.html` (+ CSS можно оставить инлайн-`<style>` на этом этапе). Режим `--check` для контроля дрейфа.
2. Смоук без нового кода рендера: спец-URL `about:chrome-preview` грузит ассет через существующий `PageSource::Static` как обычную страницу.
3. Скрин-сверка с эталоном, открытым в обычном браузере (обе темы, 4 профиля через ручную правку `data-*` в ассете). Каждое расхождение — либо BUG-NNN на движок, либо запись в известные ограничения (например, `-webkit-scrollbar`).
4. Отдельно проверить в смоуке: SVG-спрайт `<use href="#…">`, `:hover` на кнопках, `@keyframes spin`, `backdrop-filter` поповеров.

**DoD:** `about:chrome-preview` открывается и узнаваемо соответствует эталону; список расхождений оформлен (BUG-NNN / ограничения); вердикт «гипотеза подтверждена / нужны фиксы движка X,Y» записан в этот файл.

**Вердикт (2026-07-24, P1): гипотеза подтверждена, нужны 2 фикса движка.**

`about:chrome-preview` открывается через существующий `PageSource::Static` без нового кода
рендера и узнаваемо соответствует эталону: профиль-карточка, три workspace, ~10 строк вкладок
(без JS-фильтра по активному workspace — ожидаемо, `selectTab`/переключение уходит в CC-6/CC-7),
тулбар с адресной строкой, плиточная сетка быстрого доступа, ссылка «Восстановить закрытые».
Проверено `data-theme` (light/dark) × `data-profile` (personal/work) правкой атрибутов `<body>` в
сгенерированном ассете + `--screenshot`/`--dump-layout`: токены (`--surface-*`, `--accent`)
корректно каскадируются в обеих темах и профилях (сверено попиксельно, не только визуально —
первое визуальное впечатление «сайдбар остался светлым» оказалось искажением превью тёмного
скриншота, фактические пиксели `#1a1a1a`/`#121212` совпадают с ожиданием).

Найдено 2 расхождения, оба — конкретные, локализованные баги движка (не архитектурные блокеры
трека CC):

- **[BUG-333](../../bugs/BUG-333-OPEN.md)** — `.tab-row` (строки списка вкладок в `.sb-tabs`)
  рендерятся с `height:var(--tab-h)` = 0 вместо 28px (текст соседних вкладок налезает друг на
  друга). Аналогичный `height:var(--toolbar-h)` на `.toolbar` резолвится верно — баг
  специфичен для вложенности `.sidebar > .sb-tabs`, точная причина в `box_tree.rs` не
  локализована (не диагностирована до конкретной строки, репро есть).
- **[BUG-334](../../bugs/BUG-334-OPEN.md)** — SVG `<use href="#i-...">` без явных `width`/
  `height` (стандартный паттерн icon-спрайта, ~35 иконок эталона) не масштабируется к размеру
  внешнего `<svg class="icon">` (CSS 14×14) — путь красится 1:1 в координатах `viewBox`
  символа (24×24), иконки визуально искажены. Причина локализована точно:
  `box_tree.rs:1170-1180`, fallback-цепочка `vp_w`/`vp_h` не учитывает CSS-размер текущего
  viewport'а как финальный fallback (только атрибуты `width`/`height` `<use>`/`<symbol>`).
  Это прямо решает открытый вопрос CC-2 ниже: `<use>` **структурно поддержан** (резолвит
  `<symbol>`-контент в реальную геометрию пути), фикс нужен только в масштабировании — так что
  CC-2 при разборе BUG-334 может сразу чинить движок вместо build-time инлайна.

`:hover`/`@keyframes spin` из пункта 4 не проверены в этом смоуке (требуют живого окна с
курсором/анимационным циклом, а не headless `--screenshot`/`--dump-layout`) — отложено до
CC-5/CC-11, где эти механизмы вводятся предметно. `backdrop-filter` не проверялся отдельно:
уже ✅ в `CSS-SPECS.md` (GPU-компоузинг, graphic-тест 30) — не расхождение CC-1.

### CC-2 (S): SVG-спрайт

Если CC-1 показал, что `<svg><use href="#id">` движком не поддержан — выбрать и реализовать fallback: build-time инлайн каждого `<use>` содержимым `<symbol>` в `gen_chrome_assets.py` (просто и детерминированно) либо поддержка `<use>` в движке (лучше, но дороже — тогда задача уходит P1/P4 отдельным срезом, а тут остаётся инлайн). **DoD:** все ~35 иконок видны в `about:chrome-preview`.

**Вердикт (2026-07-24, P1): движок починен, build-time инлайн не понадобился.** CC-1 уже
локализовал причину до `box_tree.rs:1170-1180` — `<use>` структурно поддержан (резолвит
`<symbol>`-контент в реальную геометрию), не хватало только масштабирования к CSS-размеру
внешнего `<svg>`. Фикс — [BUG-334](../../bugs/BUG-334-FIXED.md): новая `svg_root_own_size()`
резолвит CSS width/height корня SVG (через fallback-цепочку CSS→viewBox→SVG-дефолт 300×150) в
момент построения box-дерева и прокидывает вниз по рекурсии как `own_svg_size`; `vp_w`/`vp_h`
в ветке `<use>`→`<symbol>` используют его как финальный fallback вместо `vb.width`/`vb.height`.
Подтверждено `--dump-layout about:chrome-preview`: все `SvgRoot` icon-инстансы (~35 использований)
дают `w=14.00 h=14.00` с корректно отмасштабированными дочерними `SvgShape`. Регресс-тест —
`box_tree::tests::svg_use_symbol_no_explicit_size_scales_to_css_icon_size`. Заодно фактически
закрывает DoD среза CC-CSS-5 ниже («все ~35 иконок видны без пред-инлайна») — движковая
поддержка `<use>`/`<symbol>` уже даёт это без build-time инлайна; формальное закрытие CC-CSS-5
(смена статуса в ROADMAP.md) оставлено той сессии, которая до него дойдёт по очереди
STATUS-P1.md.

## Этап A′ — CSS-доводка движка под эталон (по результатам смоука CC-1)

Gap-анализ 2026-07-23: из ~87 свойств макета движок не покрывает только перечисленное ниже. Срезы выполняются **после CC-1** — смоук даёт фактический список расхождений, отдельные пункты могут закрыться как «уже работает». Исполнитель всех срезов — **P1** (решение пользователя 2026-07-24); протокол `/lumen-add-css-property` и правила graphic-тестов обязательны как для P4-задач.

### CC-CSS-1 (S): `::-webkit-scrollbar` / `-thumb` / `-track` + `-webkit-font-smoothing`
Эталон стилизует скроллбары только через `::-webkit-scrollbar*` (строки 575–577); парсер сейчас молча роняет эти правила (варианта псевдоэлемента нет). Реализация: распознать три псевдоэлемента в `css-parser` и смаппить на уже реализованные стандартные `scrollbar-width`/`scrollbar-color` (ширина трека ← `::-webkit-scrollbar{width}`, цвет ползунка/трека ← `-thumb`/`-track{background}`; радиус — по возможности). `-webkit-font-smoothing` — parse-and-ignore (антиалиасинг растеризатора и так включён) + allowlist парс-гейта CC-3. **DoD:** скроллбары в `about:chrome-preview` соответствуют эталону; graphic-тест.

**Закрыто (2026-07-24, P1).** Оказалось не нужно трогать `css-parser`: `PseudoElementKind::Unknown` уже парсит и матчит `::-webkit-scrollbar`/`-thumb`/`-track` (case-insensitive имя), поскольку у `css-parser` нет allowlist псевдоэлементов — неизвестное имя просто хранится как строка. Новая функция `apply_webkit_scrollbar_pseudos()` ([style.rs](../../crates/engine/layout/src/style.rs)), вызываемая в конце `compute_style()`, трижды переиспользует существующий `compute_pseudo_element_style()` (тот же механизм, что и `::before`/`::after`/`::marker`) и переводит найденные декларации в `scrollbar_width`/`scrollbar_color`: ширина бакетизируется thin (≤9px) / auto (>9px) — `scrollbar-width` не имеет числового значения (CSS Scrollbars 1 §2); thumb/track-цвет применяется только если заданы ОБА (нет честного дефолта для одной недостающей стороны на уровне layout — дефолты живут в `paint::display_list`). `-webkit-font-smoothing` не потребовал вообще никакого кода — уже падает в catch-all `_ => {}` в конце `apply_declaration`, как любое нераспознанное свойство (parse-and-ignore из коробки). Аллоулист парс-гейта CC-3 не существует в коде (сам CC-3 ещё не начат) — нечего заводить заранее. 4 unit-теста (`style::tests::webkit_scrollbar_*`, `webkit_font_smoothing_is_parsed_and_ignored`) + graphic-тест 51 (новая пара демо-боксов `::-webkit-scrollbar*` vs. эквивалент на стандартных свойствах — рендерится пиксель-в-пиксель идентично). DoD выполнен: реальный `assets/chrome/chrome.html:473-475` использует ровно этот паттерн.

### CC-CSS-2 (S): `overflow: auto` — вложенные прокручиваемые панели
Хром полон внутренних скролл-зон (список вкладок сайдбара, history/bookmarks/settings, дропдауны). Статус в движке неоднозначен: клип работает, но инвентарь CSS-SPECS (строка ~179) держит «scroll ⬜ rendering» против «done» в P4-очереди (#22). Задача: проверить рендер и колёсную прокрутку **вложенных** scroll-контейнеров, довести недостающее. **DoD:** список вкладок и секции настроек прокручиваются в chrome-preview; graphic-тест на вложенный `overflow:auto`.

### CC-CSS-3 (S): `position: sticky` — scroll-follow
Sticky-слои рисуются ([renderer.rs:5680](../../crates/engine/paint/src/renderer.rs)), но следование скроллу shell-side/частично (`CAPABILITIES.md:74`). Эталон: `.site-nav`, `.net-table th`. **DoD:** sticky-элементы следуют прокрутке; graphic-тест.

**Закрыто (2026-07-24, P1).** Реальный дефект — не «частично реализовано», а неверная граница клампинга: `sticky_offset_dy`/`dx` в wgpu `renderer.rs` всегда сравнивали позицию с ГЛОБАЛЬНЫМ viewport и page-level scroll, игнорируя, что элемент может быть вложен в `overflow:auto`/`scroll`-предок (`PushScrollLayer`) со своим собственным scroll-translate в `transform_stack`. Именно это и есть `.dt-panel`→`.net-table th`/`.view`→`.site-nav` в эталоне (`.view{overflow-y:auto}`, `.dt-panel{overflow-y:auto}`) — top-level page scroll в chrome-документе почти никогда не происходит, движется только внутренняя панель, поэтому sticky просто ехал вместе с контейнером вместо прилипания ([BUG-336](../../bugs/BUG-336-FIXED.md)). **Фикс:** новый `sticky_bound()` берёт ближайший активный `clip_stack`-рект (любой overflow-предок — scroll-контейнер по CSS Overflow spec, независимо от того, кто сейчас реально скроллится) вместо глобального viewport, и мапит его обратно в pre-transform page-space через `Mat4::invert_2d_affine()` — та же screen↔page-space конвенция, что уже применяется к `PushClipRect`/`PushScrollLayer` (BUG-276/BUG-335). Без предков-клипов (top-level sticky) поведение байт-идентично прежнему. femtovg-фолбэк (не дефолтный живой бэкенд) несёт тот же баг — сознательно не тронут, задокументирован отдельно ([BUG-337](../../bugs/BUG-337-OPEN.md)). 6 новых unit-тестов `renderer::tests::sticky_*` воспроизводят именно вложенный сценарий (`cargo test -p lumen-paint --features backend-wgpu`: 1055/1055 зелёных); графический тест не добавлен — ни MCP `scroll` (игнорирует `target`), ни fragment-навигация не умеют скроллить вложенный контейнер, детерминированно заскриншотить прокрученное состояние нечем ([BUG-338](../../bugs/BUG-338-OPEN.md)). TEST-42/51/149 (существующие sticky/scroll graphic-тесты) без регрессий.

### CC-CSS-4 (M): `resize: vertical` — drag-ресайз блока
Свойство парсится, UI перетаскивания нет (CSS-SPECS:591). Нужно для DevTools-панели эталона (`.devtools{resize:vertical}`). Реализация: hit-зона у края блока с `resize`, drag меняет used-height, релэйаут. **DoD:** DevTools-панель в chrome-preview ресайзится мышью.

### CC-CSS-5 (S): SVG `<use href="#id">` / `<symbol>` в движке
Поддержка спрайта движком неизвестна до смоука CC-1. Если отсутствует — реализовать разворачивание `use → symbol` в движке (предпочтительно: убирает пред-обработку и полезно страницам) **либо** закрыть срез как n/a, признав build-time инлайн из CC-2 постоянным решением. **DoD:** все ~35 иконок эталона видны без пред-инлайна (или срез закрыт n/a с обоснованием).

**Закрыто (2026-07-24, P1): done, а не n/a.** DoD выполнен побочным эффектом CC-2/[BUG-334](../../bugs/BUG-334-FIXED.md) — движок разворачивает `use→symbol` сам, доп. реализации не потребовалось (см. вердикт CC-2 выше). Формальное закрытие статуса в ROADMAP.md выполнено этой сессией.

### CC-CSS-6 (XS): `user-select: none` / `pointer-events` — enforcement
Значения парсятся и частично прокинуты (hit-test учитывает `pointer-events:none`), enforcement выделения 🟡 (CSS-SPECS:588–589). Для хрома: текст кнопок/вкладок не должен выделяться мышью, тултипы не должны перехватывать курсор. **DoD:** выделение мышью в хроме не захватывает UI-текст.

**Закрыто (2026-07-24, P1).** `pointer-events` для хрома уже enforced бесплатно: `chrome_hit_test` (main.rs) вызывает тот же `lumen_paint::hit_test`, что и DOM-страницы, а тот пропускает бокс с `pointer-events:none` при хит-тесте (`hit_test.rs:151`, покрыто тестами `pointer_events_none_skips_box_but_descends_to_children`/`pointer_events_auto_lets_box_be_target`) — доп. код не требовался; тултип-часть DoD оказалась n/a — в `assets/chrome/chrome.html` нет отдельного tooltip-элемента (только нативные `title="…"`), перехватывать курсор нечему.

Реальный пробел был в `user-select`: во всём Lumen (не только в хроме) нет ни одного места, где мышь реально управляет drag-выделением текста — `Selection`/`Range` (`lumen-dom`) и `SelectionHighlight` (рендер `::selection`) существуют, но вызываются только из JS-шима (`window.getSelection()`), ни один обработчик мыши в `crates/shell/src/main.rs` их не трогает (проверено грепом по `extend_focus`/`caret_at_point`/`selection_rects`/`.collapse(` — 0 совпадений вне тестов и JS-шима). Буквально протестировать «мышь не выделяет UI-текст» через реальный drag поэтому нельзя — предпосылка DoD спекулятивна для всего движка, не только для хрома.

Сделано то, что реально проверяемо и корректно уже сейчас: `lumen_chrome::parse_document` подмешивает UA-дефолт `html{user-select:none}` первым правилом перед собранным `<style>`-текстом (`crates/chrome/src/lib.rs`) — только для chrome-документа (эта функция больше нигде не используется), эталон (`docs/design/lumen-v3_3.html`) не трогался (свойство не влияет на рендер, трогать замороженный файл незачем). `user-select` — наследуемое свойство, поэтому дефолт на `html` покрывает весь UI-текст; текстово он первый, так что более поздние авторские правила той же специфичности всё ещё переопределяют его (юнит-тест `ua_defaults_can_be_overridden_by_a_later_author_rule`). Второй тест (`ua_defaults_make_chrome_text_non_selectable`) проверяет на реальном ассете, что `#profileName` получает `user_select == None`. Задел на будущее: когда CC-9/CC-10 когда-нибудь введут реальный drag-select, UI-текст хрома уже будет защищён по умолчанию без доп. работы.

## Этап B — каркас

### CC-3 (M): Крейт `lumen-chrome` + build.rs кодогенерация v1

`/lumen-new-crate lumen-chrome` (зависимости: `lumen-html-parser`, `lumen-css-parser`, `lumen-dom` — также как build-dependencies). В `build.rs`:
1. Парс-гейт: `lumen_html_parser::parse` + `lumen_css_parser::parse` над ассетами; ошибка парсинга / неизвестное свойство / неподдержанный селектор = ошибка сборки (список известных исключений — `-webkit-*` — в явном allowlist).
2. Кодогенерация `OUT_DIR/chrome_gen.rs`: модуль `ids` — константы для всех элементов с `id` (типа `NodeId` не сгенерировать заранее — id узла присваивается парсером при рантайм-парсе; генерируются **строковые id + типизированный резолвер** `ChromeIds::resolve(&Document)`, заполняемый один раз при старте, паника при отсутствии id = невозможна благодаря build-гейту); enum `ChromeAction` из значений атрибутов `data-action` в разметке (атрибуты добавляются в ассеты этим же срезом: каждый интерактивный элемент эталона получает `data-action="…"` через `gen_chrome_assets.py`-маппинг); реестр `<template>`-узлов (ряд вкладки, ряд саджеста, карточка загрузки, элемент истории и т.д.).
3. Юнит-тест: резолвер находит все id на реальном ассете.

**DoD:** крейт собирается, порча CSS в ассете ломает сборку с внятной ошибкой, `cargo test -p lumen-chrome` зелёный, `subsystems/chrome.md` заведён.

### CC-4 (M): Рантайм-хост за флагом

В шелле при `LUMEN_CSS_CHROME=1`: распарсить ассеты при старте (1 раз), держать `chrome_doc: Document` + `chrome_sheet: Stylesheet` + `chrome_layout: LayoutBox`; на каждый кадр/ресайз — `layout_measured_hyp` по размеру окна CSS-px, `paint_ordered` → в начало `overlay_buf` (до legacy-панелей). Из layout читать rect элемента `#page-host` (добавить его в ассеты: контейнер, где живёт страница) → он заменяет `CHROME_H`/`page_x_offset` при активном флаге. Legacy tab-bar/toolbar при флаге не строятся; всё остальное (панели, дропдауны) пока legacy поверх. **DoD:** окно с флагом показывает движковый хром (статичный, некликабельный) + живую страницу в правильном прямоугольнике; без флага — ноль отличий; ресайз окна корректен.

**Закрыто (2026-07-24, P1).** `#contentArea` (эталонный контейнер под контент вкладки)
уже и есть искомый `#page-host` — новый id заводить не понадобилось. `lumen_chrome::
parse_document` (новая рантайм-функция крейта, зеркалирует парсинг `build.rs`) парсит
`chrome_preview::HTML` один раз при старте; `Lumen::relayout_chrome_host` гоняет
`layout_measured_hyp`/`paint_ordered` на первый известный размер окна и на каждый
`WindowEvent::Resized`. Единственная содержательная находка среза: `#contentArea`
несёт собственную демо-разметку эталона (плитки нового таба, макет сайта — контент
для автономного `about:chrome-preview`, не для наложения под настоящую страницу) и
собственный фон (`.content-area{background:var(--surface-0)}`), а его предки (`body{
background:var(--surface-1); height:100vh}`) — свой, во всё окно; поскольку хром красится
в `overlay_buf` (поверх `content`), простое обнуление детей `#contentArea` оставляло бы
и демо-плитки, и фон `body` поверх настоящей страницы. Фикс — `#contentArea` целиком
вырезается из layout-дерева (`take_layout_box_by_node`) перед покраской (rect
сохраняется в `chrome_page_host_rect` до вырезания), а сам `chrome_dl` красится не одной
копией, а через 4 клип-полосы вокруг этого rect (top/bottom/left/right) — так фон `body`
не протекает в rect страницы, а остальной хром (сайдбар, тулбар, будущие поповеры CC-9+)
рисуется как обычно. Под флагом legacy tab-bar/toolbar (и анкернутые к той же полосе
layout-toggle/settings/archive-кнопки) не строятся вовсе. Проверено вручную: `PrintWindow`-
снимками живого окна (не MCP `resource://screenshot` — тот идёт через CPU-путь, который
хром/overlay в принципе не рисует) — хром + страница видны раздельно и корректно на
дефолтном и на изменённом размере окна; без флага — байт-в-байт прежнее поведение.
`cargo test -p lumen-chrome` (14/14, +1 новый на `parse_document`), `cargo clippy
--workspace --all-targets -D warnings` и `scripts/scoped-test.sh` зелёные.

### CC-5 (M, done): Hit-test, hover/active, курсор, диспетч действий

`Lumen::point_over_chrome(x, y)` (rect-тест против `chrome_page_host_rect`) решает, чей это
пойнтер-эвент — хрома или страницы/плавающих поповеров над ней. `chrome_hit_test` гоняет
`lumen_paint::hit_test` по `chrome_layout` в оконных координатах; `chrome_action_at` поднимается по
`HitTestResult::path` (от листа к корню) до ближайшего `data-action` → `ChromeAction`;
`dispatch_chrome_action` разводит ~12 действий с реальным эквивалентом в шелле (`reload`,
`open-cert-viewer`, `toggle-shield-popover`, `toggle-find`, `open-web-sidebar`, `open-ai-sidebar`,
`toggle-downloads`, `open-print-dialog`, `toggle-devtools`, `toggle-profile-menu`, `new-tab`,
`show-view` при `data-view="settings"`) на те же функции, что вызывал легаси-тулбар; остальные ~17
(демо-only — переключение вкладок/воркспейсов/профиля, поповер-локальные тумблеры) без backing-стейта
до `ChromeModel` (CC-6) — распознаются, но no-op. `chrome_hovered_nid`/`chrome_active_nid` — отдельные
от страничных поля, кормят `:hover`/`:active` в `relayout_chrome_host` из `CursorMoved`/`MouseInput`.
Двойной учёт кликов исключён явным `if self.css_chrome_enabled { хром-путь } else { легаси
tab-strip/toolbar-путь }` в обоих обработчиках (легаси-хиттестеры раньше не красились под флагом
(CC-4), но оставались живым кодом и продолжали бы реагировать на клики/hover в той же области).
`page_offset()` — единый источник правды для смещения страницы, используется `page_point`/
`update_cursor_icon`/рендер-оффсетом (попутно исправлена задержавшаяся с CC-4 нестыковка: обе первые
функции использовали захардкоженный `toolbar::CHROME_H`/`left_dock()` вместо
`chrome_page_host_rect`). Курсор — из `HitTestResult.cursor`, приоритет выше scrollbar/страницы, когда
`point_over_chrome`. Проверено: `cargo test -p lumen-chrome` (14/14) + `cargo test -p lumen-shell`
(1695/1695) + `cargo clippy -p lumen-shell --all-targets -D warnings` зелёные; смоук-запуск
`LUMEN_CSS_CHROME=1` без падений. Автоматической прогонки реального клика/hover через OS-инжекцию
нет (в репо такого инструмента нет, а `--mcp`/IPC `click` намеренно обходит хром/панельный диспетч,
целясь только в контент страницы) — та же протокольная граница, что CC-4 (PrintWindow-снимки, не
скриншот-дифф).

### CC-6 (M): ChromeModel и мутация DOM

Модуль биндинга: снимок состояния шелла (список вкладок c workspace/контейнером/pin/спиннером, активный URL, профиль, тема, счётчик щитов, прогресс загрузок) → мутации `chrome_doc`: текст узлов, атрибуты (`data-theme`, `data-profile`, `data-layout`, классы `.active/.sleeping`), клонирование `<template>` для списков. Дифф простейший: перестроить изменённый список целиком, пометить dirty → релэйаут хрома (при необходимости `layout_mutation_incremental`). Темы/профили с этого среза управляются только `data-*`-атрибутами — Rust-`Palette` для движкового хрома не используется. **DoD:** при флаге: переключение вкладок/темы/профиля/workspace отражается в хроме; открытие вкладки добавляет ряд.

**Закрыто (2026-07-25, P1).** `ChromeModel`/`bind_model` — новый модуль `crates/chrome/src/model.rs` (decoupled от шелла, как и `lumen_a11y::chrome::ChromeTab`): `ChromeModel{dark_theme, layout_vertical, profile_slug, tabs, workspaces}`. Отклонение от брифа: эталон (`docs/design/lumen-v3_3.html`) не содержит ни одного `<template>` — «клонирование `<template>` для списков» неприменимо буквально; вместо этого узлы `#sbTabs`/`.sb-workspaces` строятся программно (`Document::create_element`/`create_text`), что надёжнее клонирования конкретной статической демо-строки эталона (нет привязки к её случайному набору классов/детей). Значки-иконки (favicon-символ, `×` закрытия) упрощены до первой буквы — визуальная доводка отложена, вне DoD. Диффа/dirty-флага нет буквально — `bind_model` дешёвый (десяток атрибутов/текстов + два маленьких rebuild-а), вызывается в начале каждого `relayout_chrome_host()`, поэтому всегда синхронизирован без отдельного состояния.

Вкладки/workspace — реальные `tab_strip`/`workspace_panel`, не демо-данные: `data-tab-id`/`data-ws-id` (штампуются `bind_model`) резолвят клик обратно в индекс/id. `dispatch_chrome_action` получил `event_loop` (нужен `close_tab`) и реализует `SelectTab`/`CloseTab`/`SelectWorkspace`/`AddWorkspace` по-настоящему (`switch_tab`/`close_tab`/`workspace_panel.set_active`/`workspaces.create`), каждый зовёт `relayout_chrome_host()` после мутации; то же самое сделано в `open_new_tab`/`close_tab`/`switch_tab` напрямую (покрывает старый хиттест тоже, не только новый chrome-диспетч) и в `close_settings_panel`/профильном поповере (тема/layout/профиль). `SetProfile`-клик в новом хроме остаётся no-op — поповер профиля ещё legacy-оверлей (CC-9/10); переключение через существующий legacy-поповер уже видно в новом хроме на следующий relayout, поскольку `bind_model` читает `profile_menu`/`dark_mode`/`vertical_tabs.visible` заново при каждом вызове. Счётчик щитов и прогресс загрузок из описания `ChromeModel` не привязаны — явное DoD-предложение их не требует, задел на follow-up.

5 новых unit-тестов в `model.rs` (`cargo test -p lumen-chrome`: 21/21) + 2 в `profile_menu.rs` (`slug_for_profile`). Реальный клик/hover через OS-инжекцию не гонялся (тот же пробел, что CC-5 задокументировал — инструмента в репо нет); верификация — ревью кода + юнит-тесты биндинг-логики + `cargo test -p lumen-shell --features v8` зелёный.

## Этап C — миграция компонентов до паритета (всё за флагом)

### CC-7 (M): Тулбар + омнибокс
Самый рискованный компонент: рендер поля, замка, star/shield — через движок; **редактирование текста остаётся существующей логикой** `address_bar` (state, IME, выделение), которая при флаге пишет значение/каретку в chrome-DOM (текст узла + каретка отдельной DisplayCommand поверх, как сейчас). Омоглиф-warning (`.omnibox-warn`) — показ/скрытие классом. **DoD:** навигация с клавиатуры и мышью полностью рабочая при флаге; скорость набора без видимых лагов (замер — в CC-12).

### CC-8 (M): Таб-бар: вертикальный сайдбар + workspaces + горизонтальный вариант
`data-layout="vertical|horizontal"`, коллапс сайдбара, дерево вкладок (`.child`, tree-lines), контейнер-полоса, спиннер (`@keyframes spin` движком), группы. Drag-and-drop вкладок — оставить legacy-механику (пиксельные вычисления поверх rect'ов из layout хрома). **DoD:** оба layout'а функциональны при флаге, включая переключение workspace и цвет `--ws-color`.

**Закрыто (2026-07-25, P1).** Основной пробел на входе: CC-6 биндил только вертикальные контейнеры (`#sbTabs`/`.sb-workspaces`) — горизонтальный layout (`data-layout=horizontal`, уже переключаемый через существующий `vertical_tabs.visible`) молча показывал статичные demo-строки эталона вместо реального состояния. Добавлены зеркальные ребилдеры `rebuild_hbar_tab_list`/`rebuild_hbar_ws_list` (`#hbarTabs`/`.hbar-ws` → `.hbar-tab`/`.hbar-ws-pill`), так что оба layout теперь отражают один и тот же `ChromeModel`. Коллапс сайдбара: новый `ChromeModel::sidebar_collapsed` + `Lumen::chrome_sidebar_collapsed`, `ChromeAction::ToggleSidebar` (был no-op с CC-5) переключает флаг и зовёт `relayout_chrome_host()`; `bind_model` ставит/снимает `.collapsed` на `#sidebar` (ширина/скрытие лейблов уже в CSS ассета с CC-3). Дерево вкладок: `ChromeTabModel::is_child` из `TabEntry::opener_id.is_some()` (7A.2) добавляет класс `.child` + `.tree-line`-коннектор — CSS эталона поддерживает только один уровень отступа, поэтому глубина > 1 сворачивается в один булев флаг (известное ограничение разметки, не движка; `tabs::tree::depth_of`/`tree_tabs.rs` для legacy-панели умеют полную глубину). Контейнер-полоса и `--ws-color`: `container_color`/`color` — `#RRGGBB`-строки (`Lumen::chrome_hex_color`, без альфы) из `ContainerKind::border_color()`/`WsEntry::accent`, пишутся как `style="background:…"` на `.container-stripe` и как custom property `--ws-color` на `.ws-item`/`.hbar-ws-pill` + фон `.ws-icon`. Drag-and-drop вкладок не тронут — уже работает поверх `chrome_layout`-rect'ов через `page_offset()`/hit-test с CC-5, отдельного кода не потребовалось. Спиннер `@keyframes spin` — статичен: тикера CSS-анимаций над chrome-документом ещё нет (CC-11), вне DoD этого среза (только структурная разметка). 8 новых unit-тестов (`cargo test -p lumen-chrome`: 28/28) + `cargo test -p lumen-shell` (1701/1701, без изменения числа — новая ветка `ChromeAction::ToggleSidebar` и поля `chrome_model_snapshot` не потребовали новых тестов сверх существующего покрытия snapshot-биндинга) + `cargo clippy -p lumen-chrome -p lumen-shell --all-targets -D warnings` зелёные. Живой смоук-запуск `LUMEN_CSS_CHROME=1` не дал падений; интерактивная проверка клика по `.sb-collapse`/переключения layout через OS-инжекцию не гонялась — тот же пробел, что CC-5/6/7 документировали (инструмента в репо нет).

### CC-9 (M): Поповеры, батч 1
Dropdown омнибокса (клоны шаблона саджеста), поповер разрешений/щита (анимированные счётчики — transitions движка или прямое обновление текста), панель загрузок, find-bar. Позиционирование — `position:absolute/fixed` в CSS хрома вместо ручных координат. **DoD:** перечисленные поповеры при флаге рендерятся движком, legacy-аналоги при флаге отключены.

**Закрыто (2026-07-25, P1).** Структурный блокер на входе: `#findBar`/`#downloadsPanel` — прямые дети `#contentArea` в эталонной разметке, а `relayout_chrome_host` (CC-4) целиком выпиливал `#contentArea` из layout-дерева перед покраской, включая все его дети — под флагом эти два поповера физически никогда бы не попали в display list, независимо от `.open`. Новый `take_content_area`/`salvage_layout_boxes` (`crates/shell/src/main.rs`) спасает конкретные узлы (по id) из выпиливаемого поддерева и пересаживает их на место `#contentArea` в дереве родителя перед покраской: `position:absolute` с явным z-index создаёт свой stacking context независимо от того, где узел висит в дереве (CSS Positioned Layout L3 §9.10) — а `#contentArea` сам не создаёт SC, — поэтому пересадка не меняет z-порядок; 2 unit-теста (`take_content_area_salvages_find_bar_and_downloads_panel`, синтетический `LayoutBox`-фикстура без окна) проверяют и сохранность спасённых узлов, и порядок в дереве. Остальные дети `#contentArea` (`#devtools`, `#cpOverlay` — CC-10) по-прежнему отбрасываются, как и раньше.

`#omniDropdown`: новый `ChromeDropdownModel`/`bind_dropdown` (`crates/chrome/src/model.rs`) перестраивает `.dd-row` из реальных `AddressBarState::suggestions()` (те же `MAX_VISIBLE`=7, что и у legacy `build_dropdown`) — иконка упрощена до цветного `.dd-icon`-свотча без клонирования inline-SVG-спрайта (тот же компромисс, что CC-6 сделал для favicon вкладок). Клик коммитит через новый `AddressBarState::commit_suggestion(idx)` (напрямую по индексу, в обход `selected_idx`, который отслеживает только клавиатурную навигацию) → `Lumen::handle_omnibox_commit`, тот же путь, что Enter на выделенной подсказке.

`#findBar`: `ChromeFindModel`/`bind_find_bar` пишет `FindState::query()`/счётчик матчей в `#findInput`/`#findCount`; легаси-бар (`find::build_bar_overlay`) выключен под флагом (подсветка совпадений на странице — `find::build_page_with_highlights` — осталась безусловной, это не хром). Найден и исправлен побочный пробел: ни `ChromeAction::ToggleFind`/`ToggleShieldPopover`/`ToggleDownloads`, ни клавиши в `handle_find_key` не звали `relayout_chrome_host()` — тот же класс бага, что CC-7 нашёл для омнибокса (движковый DOM не обновлялся без отдельного relayout-триггера); добавлены вызовы. Каретка для find-bar сознательно не добавлена: у легаси-бара её никогда не было (`find::append_bar` не рисует курсор), добавлять её здесь было бы новым поведением сверх паритета, а не миграцией существующего.

`#downloadsPanel`: `ChromeDownloadModel`/`bind_downloads` рендерит реальные `DownloadEntry` (переиспользованы `download::extension_label`/`human_bytes`, оба стали `pub(crate)`) вместо демо-карточек эталона. Per-card кнопки Open/Reveal/Cancel — вне DoD: в замороженном `docs/design/lumen-v3_3.html` они не несут `onclick` вообще (чисто декоративны в макете), тот же класс пробела, что `SetProfile` у CC-6 — заголовок панели (`#dl-close` → `toggle-downloads`) уже реально закрывает.

`#permPopover`: делит один поповер между щитом и разрешениями (как в эталоне) — `.open` следует `shields.visible || permission.visible`. `#statTrackers` — реальный `ShieldsPanel::blocked_total_count()`; `#statAds`/`#statFp` остаются эталонным `"0"` — `ShieldsPanel` хранит только один честный total (см. его же doc-comment про фабрикацию разбивки), выдумывать трекеры/рекламу/fingerprint по отдельности не стали. Два `.perm-row` эталона (Camera, Microphone — `PermissionKind::ALL`'s первые два, разметка не покрывает Notifications/Clipboard) резолвятся по позиции (`Lumen::chrome_permission_kind_for_node`, ходит вверх по родителям до `.perm-row`, затем ищет индекс среди `.perm-row`-детей `#permPopover`). Новый `PermissionPanel::set_permission` — прямая установка, а не `cycle_permission`: в эталоне две отдельные кнопки allow/deny без общей "ask"-кнопки, так что клик всегда означает конкретное значение, а не следующий шаг цикла.

14 новых unit-тестов (`cargo test -p lumen-chrome`: 35/35) + 2 новых (`cargo test -p lumen-shell`: 1703/1703) + `cargo clippy -p lumen-chrome -p lumen-shell --all-targets -D warnings` зелёные. Живой смоук-запуск/интерактивная OS-инжекция не гонялись — тот же пробел, что CC-5..CC-8 документировали (инструмента в репо нет).

### CC-10 (M): Панели, батч 2
Внутренние страницы-виды (history, bookmarks, settings — 6 секций c тумблерами/radio-cards/perm-table), командная палитра (Ctrl+K), модалы сертификата и печати, правый сайдбар (AI/Web). Возможна разбивка на 2 среза по факту. **DoD:** соответствующие legacy-билдеры не вызываются при флаге.

### CC-11 (S): Анимации и transitions хрома
Прогнать `TransitionScheduler`/`AnimationScheduler` по chrome-документу (отдельные экземпляры от страничных), подключить к кадровому циклу. **DoD:** hover-transitions, спиннер, прогресс загрузки анимируются движком.

## Этап D — качество и переключение дефолта

### CC-12 (S): Перф-гейт
Бенч (по образцу `bench_frames.rs`): время полного цикла «мутация → рестайл → релэйаут → paint» chrome-документа (~400 узлов) на hover-флип и на символ ввода. Бюджет: ≤ 2 мс на среднем железе (хром должен успевать в кадр вместе со страницей). При провале: сначала `layout_mutation_incremental` для хрома, затем — таргетные оптимизации. **DoD:** числа в этом файле + журнал; бюджет выдержан.

### CC-13 (S): A11y хрома из движкового AX-дерева
Движковое AX-дерево страницы уже есть — построить его и для chrome-документа (роли из разметки: ассетам добавить ARIA-атрибуты при генерации) и заменить синтетические узлы `lumen_a11y::chrome` (DS-17). **DoD:** MSAA-мост отдаёт хром из движкового дерева при флаге.

### CC-14 (M): Флип дефолта
По чек-листу паритета (все ежедневные сценарии: навигация, вкладки, панели, темы, DPI/zoom, split view — явный список составить в срезе) перевернуть флаг: движковый хром по умолчанию, `LUMEN_LEGACY_CHROME=1` — откат. Обновить `CAPABILITIES.md`, README. **DoD:** дефолтная сборка — движковый хром; rollback-флаг работает.

### CC-15…(серия S): Удаление legacy
По образцу S12b (V8-миграция): слайсами удалить `toolbar.rs`-билдер, `tabs/strip.rs`-рендер, DisplayList-код `panels/*`, `Palette`-константы (остаётся только то, что не покрыто макетом — см. риск №5). Финал — удалить rollback-флаг.

## Этап E — компайл-тайм оптимизации (опционально, после флипа)

### CC-16 (M): Пред-парсинг при сборке
Сериализация `Document`+`Stylesheet` хрома в build.rs (serde-фича на dom/css-parser типах или кодогенерация конструкторов) + `include_bytes!` → пропуск парсинга при старте. Ценность — миллисекунды старта и отсутствие парсера в критическом пути первого кадра; делать только если профиль старта это оправдает.

### CC-17 (L, research): Разрез конвейера «после матчинга»
Пред-вычисленные списки деклараций на узел (+ альтернативные наборы для `:hover/:active`, тем и профилей) и новый вход в layout, принимающий их вместо `Stylesheet`. Требует расчленения `compute_style`/`build_box` — заводить только при доказанной перф-нужде (CC-12), иначе закрыть как «не оправдано».

---

## Риски и открытые вопросы

1. **Омнибокс** — редактирование текста, IME, каретка через движок не делаем (осознанно): гибрид «движок рендерит, address_bar редактирует». Риск рассинхрона метрик текста — проверять в CC-7.
2. **SVG `<use>`** — поддержка движком неизвестна до CC-1; fallback (build-time инлайн) дешёвый.
3. **Перф полного рестайла** на каждый hover/символ (~400 узлов, каскад всегда полный) — главный количественный риск; ворота CC-12, страховка — инкрементальный layout и CC-17.
4. **Смена текстового пути хрома**: сейчас текст хрома живого окна рисует femtovg по ручным координатам; движковый хром меряет текст через layout-мершуер — метрики/переносы могут отличаться от legacy на 1–2 px. Это ожидаемо (паритет с эталоном важнее паритета с legacy).
5. **Хром-функциональность вне макета** (reader view, source view, split view, hints, spellcheck-меню, реальные DevTools-панели, ~30 панелей — макет покрывает не все): остаются legacy-оверлеями поверх движкового хрома неограниченно долго; мигрируются по мере расширения эталона новыми версиями.
6. **Два независимых набора interactive-thread-locals не существует** — один на процесс: ставить/чистить `set_interactive_state` строго вокруг каждого прохода (страница и хром — разные проходы) — дисциплина, закрепить комментом в хосте CC-4.
7. **DPI/zoom/split view** — layout хрома в CSS-px по масштабу окна; проверяется в чек-листе CC-14.
