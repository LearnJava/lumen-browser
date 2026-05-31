# Lumen — браузер на Rust с собственным движком

> **Lumen** (лат. *свет*, единица светового потока) — приватный, лёгкий, прозрачный браузер. Имя отражает философию проекта: показывать пользователю всё, что происходит, и не быть тяжелее, чем нужно.

## 🔄 В работе сейчас

Задачи, взятые в работу параллельными сессиями. **Не дублировать.** Подробнее о протоколе — в `CLAUDE.md`, раздел «Координация параллельных сессий».

Над проектом параллельно работают **3 программиста** (P1–P3). Раскрой задач по программистам и доменные зоны — в `CLAUDE.md`, раздел «Распределение задач между программистами». Если в сессии тебе сказали «ты программист N» — твои задачи помечены `[PN]` в разделе «Roadmap — приоритизация задач» этого файла.

Формат строки резервации: `- 🔄 <имя задачи> [PN] — <имя ветки> — <YYYY-MM-DD>`.

- 🔄 SUBSYSTEMS.md: split per-crate + translate to English [P3] — subsystems-split — 2026-05-19

## Статус реализации

**Текущая фаза:** Phase 1 (активная разработка). **Phase 0 закрыта 2026-05-26** — все P1-крейты (html-parser, css-parser, layout) и P2-крейты вышли из прототипного состояния; engine открывает `samples/page.html` с полным block/inline/flex/grid layout, CSS cascade, positioned layout, transitions, transforms. Этот блок обновляется при каждом коммите, реализующем пункт плана. Условные обозначения: ✅ готово · 🟡 в работе · ⬜ запланировано.

### Инфраструктура
- ✅ Cargo workspace, 10 крейтов
- ✅ `rust-toolchain.toml` (stable + rustfmt + clippy)
- ✅ `.gitattributes` (LF в репо, кросс-платформенные line endings)
- ✅ Ветка `main`, локальные коммиты, без remote

### Крейты
- ✅ `lumen-core` — типы и trait-ы: `Error`, **структурированный `Url` (scheme/host/port/path/query/fragment + serialized cache, methods host_ascii/effective_port/path_and_query/resolve)**, `Event`, `Capability`, `Module`, геометрия (`Rect`, `Point`, `Size`), `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource` (загрузчик rules text), **`RequestFilter`** (per-URL `should_block(&Url) -> Option<String>`), `EncodingDetector`, **`EventSink`** (`emit(&Event)`, приёмник `Event::Request*` из подсистем), **`DnsResolver`** (`resolve(hostname, port) -> Result<Vec<SocketAddr>>`; trait-точка для system / cached / DoH / DoT резолверов), **`HstsEnforcement`** (`is_https_only(host, now_unix) -> bool` + `record_sts(host, max_age, include_subdomains, preload, now_unix)`; без `Result`, fail-open; реализация в `lumen-storage::hsts::HstsStore`, потребитель — `lumen-network::HttpClient::with_hsts(...)` для RFC 6797 http→https upgrade и persist `Strict-Transport-Security`), **`HttpCredentialProvider`** (`credentials(&HttpAuthChallenge { origin, realm, scheme: HttpAuthScheme::{Basic|Digest} }) -> Option<HttpCredentials { username, password }>`; HTTP auth по RFC 7617 / RFC 7616, потребитель — `lumen-network::HttpClient::with_credentials(...)`), **`JsRuntime`** (eval / set_global / get_global / call_function через `JsValue` JSON-совместимые типы; `NullJsRuntime` stub возвращает `JsError::NotImplemented`; первая реальная реализация — QuickJS / rusty_v8). **Sprint 0 P3 trait-anchors** (interface готов, Null-stub-ы — «не поддерживается»; интегрируются через provisional-крейты по §5): **`UnicodeProvider`** (UAX #14 line-break / UAX #29 segmentation / UAX #9 bidi — под `icu4x.segmenter` + `icu4x.linebreak`), **`IdnaProvider`** (UTS #46 to_ascii/to_unicode — под `idna`-crate), **`PublicSuffixList`** (eTLD / eTLD+1 / is_public_suffix для cookie matching + Safe Browsing host-suffix — под `publicsuffix`-crate, P3 п.2B), **`ContentDecoder`** (HTTP `Content-Encoding` encoding+decode; есть `UnsupportedContentDecoder { encoding }` stub возвращает `Error::Other` — под расширения `brotli-decompressor` / `ruzstd` / `zstd-safe`, P3 п.1A), **`FontFormat`** (format_name/can_decode/decode_to_sfnt — под `woff2` для WebFonts), **`SpellChecker`** (check/suggest/locale — под `hunspell-rs` / `spellbook`; `NullSpellChecker::check` всегда true, чтобы UI не подчёркивал всё), **`HyphenationProvider`** (hyphenate/locales — под `hyphenation` с TeX-словарями). Модули **`punycode`** (RFC 3492 encode) + **`idn`** (`domain_to_ascii`) для IDN-доменов
- ✅ `lumen-dom` — арена + `NodeId` + `Document/Node/NodeData`, API: create/append/detach/Display, **`DocumentMode` enum + `Document.mode/set_mode`** (HTML5 §13.2.6.2 — выставляется tree builder-ом по DOCTYPE, см. `quirks_mode`), **`Document.target_id` + `target()`/`set_target(Option<S>)`** (CSS Selectors L4 §9.6 + HTML LS §7.10.6 — id из URL fragment для `:target` matcher-а; setter фильтрует empty string в `None`; выставляется shell-интеграцией P3 при навигации, не tree builder-ом), 30 тестов (включая кириллицу)
- 🟡 `lumen-shell` — точка входа: три режима (пустое окно / файл / URL). Внешний CSS через `<link rel=stylesheet>`: загружается с диска (относительно HTML-файла) или по сети (относительно базового URL). Bundled Inter-Regular.ttf через `include_bytes!`. **HTML event loop framework + integration в winit-loop + task source priorities + requestIdleCallback** в `lumen-shell::runtime`/`Lumen` — per-source TaskQueue (`[VecDeque; 7]`, обход в `TaskSource::PRIORITY_ORDER`: `UserInteraction > DomManipulation > HistoryTraversal > Networking > Timer > Rendering > IdleTask`) + MicrotaskQueue (drain-all) + EventLoop::step, rAF с cancel, idle-callback-и с `IdleDeadline {time_remaining, did_timeout}` + опциональным `timeout_ms` (абсолютный override caller-budget), observer registries (Resize/Intersection/Mutation), reentrancy через `Rc<RefCell>` + `EventLoopHandle::clone`; Lumen дёргает `about_to_wait → step()×N` (cap 256) + `run_idle_callbacks(remaining_ms, now_ms)` с фиксированным бюджетом `IDLE_BUDGET_MS=10.0` когда дошли до `StepResult::Idle` (или 0 ms если упёрлись в cap=256 — тогда срабатывают только timeout-callback-и), `Resized → deliver_observer_records(Resize)`, `RedrawRequested → run_rendering_step(timestamp_ms)` перед render. 34 unit-теста runtime. **Find in page (Ctrl+F)** в модуле `lumen-shell::find`: поиск по `DrawText`-командам display list через `TextMeasurer` (case-insensitive, Unicode-aware, non-overlapping), `FindState { open, query, active }` с next/prev cycling, `build_with_overlay` вставляет FillRect-подсветки перед своими `DrawText` (active=оранжевый, inactive=жёлтый) + UI bar (Найти / input / counter) в правом верхнем углу. Ctrl+F открывает, Esc закрывает, Enter/F3=next, Shift+Enter/Shift+F3=prev, Backspace стирает, остальные символы из `KeyEvent.text` идут в query. Reload сбрасывает find (display list другой). **Scroll-to-match** — `find::scroll_to_match(match_rect, viewport_height, current_scroll) -> Option<f32>` (pure-fn) и `Lumen::scroll_to_active_match` вызываются после next/prev/backspace/text-input: если активный матч уже целиком в viewport — no-op; иначе скролл выставляется так, чтобы match сидел в верхней четверти окна (`SCROLL_MARGIN_FRACTION = 0.25` — компромисс между alignment-top и центрированием). Caller обязан clamp-нуть в `[0, max_scroll]` после возврата (функция знает только viewport-геометрию, не content_height). 32 unit-теста find (+8 на scroll_to_match). **Scroll-state** (Lumen, `scroll_y` + `content_height`): MouseWheel (LineDelta 40 CSS px / PixelDelta /DPR), стрелки ↑/↓ (40 px, auto-repeat на repeat-event-ах), PageDown/PageUp/Space (90% viewport, Shift+Space = вверх), Home/End. `clamp_scroll` держит scroll в `[0, max(0, content_height − viewport_height)]`. `Renderer::render(content, overlay, scroll_y)` — page-полоса display list-а получает `-scroll_y` Y-offset, overlay-полоса (find-bar) рисуется без смещения = viewport-locked. Reload и load новой страницы сбрасывают scroll в 0. 9 unit-тестов scroll (clamp / content_height / keybindings). **Vertical scrollbar overlay** (`lumen-shell::scrollbar`): pure-fn `build_scrollbar_overlay(scroll_y, content_height, vw, vh) -> DisplayList` возвращает 2 FillRect (track + thumb у правого края, 8 px ширина, alpha 28 / 120); скрыт когда контент помещается в viewport. `thumb_geometry(scroll_y, ch, vh) -> (top, height)`: `thumb_h = max(MIN_THUMB_HEIGHT=24, vh²/ch) clamp до vh`, `top = (vh − thumb_h) × (scroll_y / max_scroll).clamp(0, 1)` — линейный mapping даже после clamp-а к минимальной высоте достигает endpoint-ов корректно. `RedrawRequested` подмешивает scrollbar в overlay-буфер ПЕРЕД find-bar-командами (painter's order, не пересекаются по x). **Drag + track-click**: `scrollbar::classify_track_click(x, y, scroll_y, ch, vw, vh) -> TrackClick { None | Thumb | Above | Below }` — единая точка решения для MouseDown. Thumb → старт `ScrollDrag { start_scroll_y, start_mouse_y }` + `scroll_for(current_mouse_y, ch, vh) = start_scroll_y + Δy × max_scroll / (vh − thumb_h)` (без clamp, caller вызывает `scroll_to`-обёртку которая клампит); Above/Below → page-jump на ±`page_step(vh)`; None → клик мимо. Lumen хранит `cursor_position` (физические px, конверт через DPR) + `scroll_drag: Option<ScrollDrag>`; MouseUp / reload сбрасывают drag. 29 unit-тестов scrollbar (12 overlay/thumb + 9 classify_track_click + 8 ScrollDrag). **Smooth-scroll** (`lumen-shell::scroll_anim`): keyboard / wheel / page-jump (включая track-click) / find-scroll-to-match плавно анимируются — `ScrollAnim { start_y, target_y, start_time_ms }`, out-cubic easing, фиксированная длительность `DURATION_MS=200.0` ms. `Lumen.start_smooth_scroll(target)` cancel-ит активную анимацию и стартует новую; `scroll_by_smooth(delta)` аддитивен поверх текущего target-а (repeat-wheel/keys не «откатывает» к старту). `advance_scroll_anim()` тикается перед каждым `RedrawRequested` render-ом и просит ещё один redraw до завершения. Drag thumb scrollbar-а остаётся instant. 12 unit-тестов scroll_anim (endpoints / monotonicity / midpoint / before-start / past-end / quarter-decelerates / backwards-anim). **Cursor-icon feedback на hover scrollbar thumb** (`Lumen::update_cursor_icon`): при hover над thumb-ом winit cursor → `CursorIcon::Pointer` (сигнал «интерактив»), вне scrollbar-а / над пустым track-ом → `Default`. Drag фиксирует Pointer пока зажата кнопка, даже когда курсор уходит за пределы окна (winit продолжает CursorMoved-events). Pure-fn `cursor_icon_for_hover(TrackClick, drag_active)` тестируется отдельно (5 unit-тестов). `last_cursor_icon` кэширует предыдущее значение → не дёргает FFI без необходимости. **Link click navigation** (`lumen-shell::links`): `find_link_href(doc, node_id)` ходит вверх по DOM-цепочке от hit-tested узла до ближайшего `<a href>` (HTML5 activation behavior); `is_navigable_href` фильтрует `javascript:`/`mailto:`/fragment-only; вставлен в MouseInput-Pressed после form-dispatch — единственный hit_test переиспользуется; `PageSource::resolve_href` делегирует к `ResourceBase::resolve_str`. **Fragment-only hrefs** (`#id`) обрабатываются `navigate_fragment` (не cross-page navigation): `is_navigable_href` → false, `fragment_only` возвращает id, `find_element_by_id` ищет по DOM, `navigate_fragment` обновляет `doc.set_target` + `relayout` + `scroll_to`. 12 unit-тестов. Phase 0 ограничения: reload через queue_task отложен (требует Rc<RefCell<Lumen>>-рефакторинга под JS engine), горизонтальный scroll + momentum (free-flick на trackpad) пока без реализации.
- ✅ `lumen-html-parser` — минимальный токенизатор (Data/Tag/Attribute/Comment, **расширенный набор ~250 named entities** через сортированную const-таблицу + numeric, **RAWTEXT для `<script>`/`<style>`**, **RCDATA для `<title>`/`<textarea>`**, **DOCTYPE с PUBLIC/SYSTEM** — public_id/system_id как `Option<String>` чтобы различать missing/empty) + lenient tree builder. **Модуль `quirks_mode::detect_document_mode` (HTML5 §13.2.5.1)** — exact-match public/system IDs + ~55 prefix-правил + HTML 4.01 Frameset/Transitional + XHTML 1.0 правила; tree_builder применяет detection при первом DOCTYPE-токене, при отсутствии DOCTYPE — Quirks (§13.2.6.4.1). **Модуль `srcset` (HTML5 §4.8.4.3.5 + §4.8.4.3.7 + §4.8.4.4)** — `parse_srcset` / `pick_best_for_density(dpr)` + `parse_sizes` / `evaluate_sizes(viewport)` / `pick_best_for_width(source_size_px, dpr)` для `<img srcset sizes>` / `<source srcset>`; density (`Nx`) и width (`Nw`) descriptors, sizes-атрибут с media-condition (min-/max-width|height + orientation + **prefers-color-scheme**, AND-list, **ведущий `not` для инверсии clause** — Media Queries L4 §3.2, lenient-вариант «not ко всей AND-цепи»; **L4 nested-формы `(not <cond>)` и `((<cond>))`** внутри clause-скобок — strict per spec `<media-not> = not <media-in-parens>` через `MediaClause::Nested(Box<MediaCondition>)` + paren-aware top-level split-by-`and`); SizesViewport `{width_px, height_px, root_font_size_px, prefers_dark}` + SizeLength (Px/Em/Rem/Vh/Vw/Vmin/Vmax/Percent); `parse_media_condition` экспортирован публично для re-use в picture-picker-е. **Модуль `picture` (HTML5 §4.8.4.4)** — `pick_picture_source(doc, picture_node, &PictureParams)` walks `<source>` детей `<picture>` в source-order с фильтрами по `type` (case-insensitive lookup в `supported_types: Option<&[&str]>`; `None` отключает фильтр; пустой `type=""` = match-everything) и `media` (тот же media-AST что у sizes), pick через srcset+sizes (width-picker для Nw, density для Nx), fallback на первый `<img>` ребёнок; `pick_img_source` — отдельный entry для одиночного `<img>` (srcset+sizes → src; пустой `src` → None). `PickedSource { url, intrinsic_width: Option<u32>, intrinsic_height: Option<u32> }` — author-объявленные dimensions из `<source|img width|height>` для CLS-protection (HTML5 §10 «mapped attributes»; `parse_dim_attr` — leading digits, отрицательные/percent отбрасываются). **Модуль `preload_scanner` (HTML LS §13.2.6.4.7)** — `scan_preload_hints(&str) -> Vec<PreloadHint>` бежит поверх Tokenizer-а без построения DOM и эмитит `Stylesheet`/`Script`/`Image`/`SourceSet`/`Preload`/`Preconnect` hints в source-order; `rel` multi-token, RAWTEXT внутри `<script>`/`<style>` корректно не парсится как теги. **P1 п.3B — push-tokenizer + incremental tree builder**: `PushTokenizer::feed(&str)` / `end() -> Vec<Token>` — обёртка над тем же pull-Tokenizer-ом, поверх owned-`String`-буфера + эвристики `find_safe_split` (учитывает `<!--…-->`, `<!DOCTYPE…>`, `<tag…>`, `&entity;` в data state и в RCDATA). Pull-Tokenizer изменён: при EOF в text-only loop восстанавливает `text_only`-поле (раньше `.take()` терял state в push-режиме). Публичные `Tokenizer::with_state(input, text_only)` + `pos()` + `text_only_state()` для возобновления между chunk-ами. `IncrementalTreeBuilder { feed(&str), finish() -> Document }` — push-вариант `parse()`-функции, держит `Document`+stack+seen_doctype между вызовами и применяет токены через общий с `parse()` `apply_token`-helper. **Инвариант: pull и push дают побайтово равный DOM** — обеспечивается text-node coalescing в `apply_token` (если последний ребёнок — text, дописываем; pull выдаёт цельный Text-токен, для него no-op). UTF-8: caller отвечает за code-point boundary в chunk-ах. Разблокирует P3 п.4B (streaming pipeline). 335 тестов. Отложено: CDATA, полный набор named entities (~2125 имён в spec), legacy без `;`, insertion modes, application Quirks-mode переключений в layout/cascade, `calc()`/`min()`/`max()` в length-значениях sizes, `loading="lazy"` через IntersectionObserver.
- ✅ `lumen-css-parser` — расширенные селекторы: simple (type/class/id/universal/attribute/pseudo), compound (`p.foo#bar`), complex с combinator-ами (` `, `>`, `+`, `~`); attribute-операторы `=`, `~=`, `|=`, `^=`, `$=`, `*=` с **ASCII case-insensitive флагом `[a=v i]`** (CSS L4 §6.3.6); structural pseudo-classes (`:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`); функциональные pseudo (`:nth-child(an+b [of <selector-list>])`, `:nth-last-child(an+b [of <selector-list>])` — `of` clause из CSS Selectors L4 §6.6.5.1 фильтрует sibling-pool до nth-индексации; `:nth-of-type`, `:nth-last-of-type` с ключевыми словами `odd`/`even`; **CSS Selectors L4 `:not(selector-list)`** — §5.4, selector-list с combinator-ами и nested `:not(:not(...))`, specificity = max-of-list; **CSS4 `:is(selector-list)` и `:where(selector-list)`** — selector-list внутри, specificity = max-of-list для :is, 0 для :where); **form-state pseudo (CSS Selectors L4 §14.2/§15.4/§15.5, HTML5 §4.10.3/§4.10.19/§4.16.4)** `:required`/`:optional`/`:read-only`/`:read-write`/`:disabled`/`:enabled` — pure attribute-based matcher-ы в layout (fieldset disabled-наследование с исключением первого `<legend>`-ребёнка, option наследует disabled от optgroup, read-only по умолчанию для не-form элементов per spec, contenteditable inheritance для read-write); **UI-state pseudo (CSS Selectors L4 §10.1/§10.2/§10.4, HTML5 §4.16.3)** `:checked`/`:indeterminate`/`:default` — pure DOM-based matcher-ы в layout (checkbox/radio через `checked`-атрибут, option через `selected`; radio-группа indeterminate через scope ближайшего `<form>`-предка с проверкой single-checked по `name`; default-submit для первой submit-кнопки внутри формы; checkbox indeterminate всегда false без runtime form-state); **`:lang(<language-tag>#)`** (CSS Selectors L4 §11) — функциональный с comma-list BCP 47 tags, matcher по RFC 4647 basic filtering против content-language (`lang`/`xml:lang` атрибут + наследование от ancestor-ов; `lang=""` — «явно неизвестен», не наследует); **`:dir(ltr|rtl)`** (CSS Selectors L4 §13.2) — functional pseudo с `DirArg::Ltr|Rtl` enum-аргументом, matcher walking up parents по `dir`-атрибуту с HTML5 §3.2.6.1 fallback на `ltr`; `dir="auto"` в Phase 0 без UAX #9 first-strong scan трактуется как `ltr` (real auto-direction отложен до bidi-движка); **link pseudo (CSS Selectors L4 §6.2)** `:link`/`:visited`/`:any-link` — pure DOM-based matchers в layout: `:any-link` и `:link` ↔ `<a>`/`<area>`/`<link>` с `href`-атрибутом (HTML5 §4.6.1 hyperlink), `:visited` всегда `false` (Phase 0 без history-runtime, privacy-safe default); **`:scope`** (CSS Selectors L4 §4.2) — root of selector matching context; в author-CSS без querySelector-runtime matches document root element (эквивалент `:root`); **`:target`** (CSS Selectors L4 §9.6) — pure DOM-based matcher: element с `id` равным `Document::target()` (URL fragment без `#`, case-sensitive per HTML LS §3.2.6); functional-формы `:target(x)` отбрасываются в `Unsupported`. Shell-интеграция: `parse_and_layout` устанавливает `doc.set_target(fragment)` из URL перед layout; клики `<a href="#id">` вызывают `navigate_fragment` (обновляет target + relayout + scroll) — P3 done 2026-05-25; **`:target-within`** (CSS Selectors L4 §9.7) — element сам `:target` ИЛИ has-descendant с `:target`; реализация — `matches_target_within` short-circuit при `Document::target() == None`, иначе свой `id` + `any_descendant` обход поддерева; без зависимости от matcher-а `:has`; **`:defined`** (CSS Selectors L4 §6.4.1, HTML LS §4.13.5) — pure DOM-based matcher: built-in HTML/SVG/MathML элементы и зарегистрированные custom elements. В Phase 0 без `CustomElementRegistry` matcher использует аппроксимацию по HTML LS §4.13.2: имя custom-element-а обязано содержать ASCII `-`, поэтому `defined = !name.contains('-')`. Поддерживает FOUC-protection idiom `:not(:defined) { display: none }`; **open-state pseudo (Fullscreen API §4.2, CSS Selectors L4 §16.5.2, HTML LS §6.12.2)** `:fullscreen`/`:modal`/`:popover-open` — runtime-only state, Phase 0 без shell-интеграции matchers всегда `false` (privacy-/UX-safe default; нельзя имитировать fullscreen/modal/popover-стили вне реального runtime), variant-ы дают правильный specificity-count и `:not(:fullscreen)`-идиомы; **time-dimensional pseudo (CSS Selectors L4 §11.4)** `:current`/`:past`/`:future` — для timed-text/WebVTT cue rendering, runtime-only (media timeline + cue lifecycle), Phase 0 matchers всегда `false`; interactive (`:hover` и т.д.) парсятся, не матчат; pseudo-elements `::name` (парсятся, не матчат). Specificity по CSS Selectors Level 3+4. **`!important` флаг в `Declaration`** (CSS Cascade L4 §8.1). **Custom property declarations (`--name: value`)**. **`@property` rules** для регистрации custom properties с syntax/inherits/initial-value. **`@media` rules** (Media Queries L4): MediaQuery с OR-list `MediaQueryClause { negated, conditions }`, MediaCondition (MediaType / Feature / Unsupported), MediaFeature (min/max-width/-height, orientation, prefers-color-scheme); ведущие `not` (инверсия clause) / `only` (L3 backcompat no-op) распознаются с whitespace-/`(`-границей (чтобы `notepad` не разваливался). `Unsupported` под `not` остаётся unknown = false (spec §3.2). 119 тестов (+5). Отложено: namespace prefix, типизированные значения деклараций других видов (length / color / calc — типы хранятся в layout)
- ✅ `lumen-layout` — block-flow + **inline-flow** + **replaced (`<img>`)** с specificity-based style cascade, **CSS-wide keywords (inherit / initial / unset / revert по CSS Cascade L4 §7)** и line wrapping: compound и complex selectors (combinators, attribute, structural и функциональные pseudo, `:not`), наследование (color, font-size, line-height, text-align, text-decoration), color (полный CSS3 набор из 147 named colors + rebeccapurple из CSS4 + transparent + hex 3/4/6/8 digit + rgb/rgba/hsl/hsla с modern syntax), display (block/inline/none), margin/padding (включая shorthand), text-align (left/center/right), text-decoration (underline / overline / line-through, можно комбинировать, `none` сбрасывает; + L3 longhands `text-decoration-style` (solid/double/dotted/dashed/wavy) и `text-decoration-thickness` (auto/from-font/`<length>`/`<percentage>`) — рендеринг в lumen-paint реализован для Solid/Double/Dotted/Dashed/Wavy (Wavy через серию узких axis-aligned столбцов с sin-смещением, амплитуда `1.5·t`, длина волны `4·t`, sample step `max(1, t·0.5)`)), **text-wrap (CSS Text L4 §6.4)** — два longhand-а (`text-wrap-mode`: `wrap|nowrap`, `text-wrap-style`: `auto|balance|stable|pretty`, оба inherited) и shorthand `text-wrap` с грамматикой `<'text-wrap-mode'> || <'text-wrap-style'>` — Phase 0 parsing+storage ✅; `text-wrap-mode: nowrap` связан с inline-flow; `balance` (binary-search по wrap_width, CSS Text L4 §6.4.2) и `pretty` (widow prevention — сдвиг последнего слова предпоследней строки) реализованы в `box_tree::balance_wrap/pretty_wrap`; `stable` ≡ `auto` для статического layout; 9 unit-тестов (P1 п.5), **background-origin / background-clip (CSS Backgrounds L3 §3.7-§3.8 + L4 для `text`)** — два non-inherited keyword-свойства (`border-box | padding-box | content-box`, плюс `text` у clip), Phase 0 parsing+storage (реальный выбор box-edge для tile-тиления и обрезки фона — задача P2), **width / height (px)**, **border (width/style/color, все shorthands, box model)**, **box-sizing (content-box / border-box)**, **CSS Variables L1 (`--name` + `var()`)** — `ComputedStyle.custom_props: HashMap`, inherited по спеке; отдельный custom-pass между font-size pre-pass и main-pass применяет все `--name: value` декларации с уважением к specificity / !important / source order; в main-pass `var(--name [, fallback])` разворачивается рекурсивно в value перед стандартным парсингом свойства (depth limit 32, циклы дают «invalid at computed value time» — declaration ignored), **CSS math-функции (Values L4 §10, §10.6, §10.7-10.9)** — `Length::Calc(Box<CalcNode>)` с базовыми (Add/Sub/Mul/Div/Min/Max/Clamp) и `Func(MathFn, args)` для 17 научных функций (sin/cos/tan/asin/acos/atan/atan2/pow/sqrt/exp/log/hypot/abs/sign/mod/rem/round); recursive-descent парсер с приоритетами `*//` > `+-`, скобки, унарный минус, nested function calls; angle-units (deg/rad/turn/grad) лексер конвертирует в радианы; работает с любыми поддерживаемыми единицами + unitless для умножения; поверх var()-substitution (`width: min(var(--w), 50px)`, `width: calc(pow(2, 5) * 1px)`). Length-units: px, em, rem, % (em/rem/% для font-size и line-height; % в margin/padding пока игнорируется до containing-block). `TextMeasurer` trait + `layout_measured()` для word-wrap по реальным шрифтовым метрикам. `InlineRun` объединяет текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`, `<strong>`, и т.д.) в один поток строк с per-сегментными стилями; каждый `InlineFrag` хранит свою ширину для align_lines и подрисовки декорации. `serialize_layout_tree` + golden snapshot-тесты (`UPDATE_SNAPSHOTS=1` для регенерации). **Sprint 0 контракты P1**: модули `stacking` (`StackingContextId`/`PaintPhase`/`PaintOrder`/`StackingTree`), `property_trees` (`TransformTree`/`ScrollTree`/`EffectTree`/`ClipTree` + `Mat4` + `PropertyTreeNodeId`), `animation` (`AnimValue` + `AnimationInterpolator` trait + `NoopInterpolator` step-half stub + `LinearInterpolator` + **`AnimValue::TransformList(Vec<TransformFn>)` с matched-pair + 2D matrix decompose fallback** по CSS Transforms L2 §15 + **`AnimValue::FilterList(Vec<FilterFn>)` с matched-pair lerp + lacuna-padding по CSS Filter Effects L1 §6**) — interface-first типы для P2 compositor / painting order и для P1 п.2A/2B/3A. **P1 п.2B — Property trees построение**: `PropertyTrees::build(&LayoutBox)` обходит layout pre-order и собирает четыре независимых дерева (Transform / Scroll / Effect / Clip). Триггеры: TransformNode — `transform != []` (локальная Mat4 = T(origin)·M·T(-origin)); ScrollNode — `overflow-x/y != visible`; EffectNode — `opacity<1` ∨ `filter` ∨ `mix-blend-mode != normal` ∨ `isolation: isolate`; ClipNode — `clip-path` ∨ `overflow-x/y` clipping. Mat4 расширен 2D-builders (translation/scale/rotate/skewX/skewY/matrix) + column-major multiply + **`invert_2d_affine()`** (через det(a·d - b·c); сингулярные → None) + **`transform_point_2d(x, y)`** (для hit testing P2 п.2B). Parent-граф каждого дерева — независимый (ближайший ancestor, который сам вкладывал узел в *это* дерево; иначе root). Анонимные InlineRun-ы пропускаются. P2 переходит с `PropertyTrees::build_stub()` на `::build()` без правок API. **P1 п.2A — Stacking contexts impl**: `StackingTree::build(&LayoutBox)` обходит layout pre-order и собирает SC по CSS Positioned Layout L3 §9.10 (триггеры: `position: fixed|sticky` всегда; `relative|absolute` с явным z-index; `opacity<1`; `transform`/`filter`/`clip-path` ≠ none; `mix-blend-mode` ≠ normal; `isolation: isolate`; `will-change` с stacking-property). Дочерние SC sortируются stable по z (`auto` ≡ 0). `ComputedStyle` расширен: `position`, `z_index: Option<i32>`, `isolation`, `mix_blend_mode` (16 keyword-ов из CSS Compositing & Blending L1 §3.1) — non-inherited, парсеры + ветви CSS-wide keyword-ов. Анонимный InlineRun не учитывается как owner SC (защита от фантомных контекстов). **CSS Quirks Mode UA-rule для `<table>`**: `apply_quirks_table_reset` в `compute_style` читает `doc.mode()` и при `DocumentMode::Quirks` сбрасывает у `<table>` font / color / text-align / white-space к initial-values (как в Chromium/Firefox/WebKit). В Standards / LimitedQuirks не применяется. Author CSS поверх — выигрывает. Отложено до появления table/inline-block layout: table cell width quirk, IE7 line-height quirk, unitless length quirk, flex/grid, float, font-weight/style на inline-уровне. **CSS Positioned Layout L3**: `top`/`right`/`bottom`/`left`/`inset` в ComputedStyle (initial=auto, non-inherited); `position: relative` — `shift_tree` visual offset; `position: absolute/fixed` — out-of-flow (`abs_deferred`) + `lay_out_abs_children` (двухпроходный: layout в (0,0) → shift по PCB); PCB threading (`pcb: Rect`) через `lay_out`/`lay_out_flex`/`lay_out_grid`. 9 unit-тестов. **`position: sticky` algorithm stub** — `StickyBox` + `collect_sticky_boxes()` + `compute_sticky_offset(scroll_x, scroll_y, vp_w, vp_h) → (dx, dy)` pure-fn; non-px inset → None; дедупликация по `NodeId`; 9 unit-тестов. CSS wiring — `STATUS-P4.md` "Needs wiring". **Image viewport-gating** — `gate_image_requests(root, viewport, scroll_x, scroll_y) → HashSet<NodeId>` в `image_gating` module; AABB intersection viewport ± 2 экрана; 7 unit-тестов. **CSS Scroll Snap L1 algorithm stub** — `SnapPoint` + `SnapContainer` + `collect_snap_containers(root) → Vec<SnapContainer>` + `find_snap_target(container, current_scroll, target_scroll) → Option<(f32,f32)>` в `lib.rs`; mandatory vs proximity strictness; `scroll-snap-stop: always` barrier semantics; NodeId deduplication; 10 unit-тестов. CSS-fields already in ComputedStyle (P4); P3 shell integration → `STATUS-P4.md` "Needs wiring". **CSS Scroll-Driven Animations L1 algorithm stub** — `ScrollTimeline`/`ViewTimeline`/`NamedScrollTimeline`/`NamedViewTimeline`/`ScrollAxis`/`Viewport` в `scroll_timeline.rs`; `resolve_scroll_progress` (скролл-прогресс контейнера [0.0, 1.0], Block/Inline/X/Y axis, root viewport или конкретный элемент) + `resolve_view_progress` («cover» range — прогресс видимости элемента в viewport [0.0, 1.0]); `collect_named_scroll/view_timelines()` — стабы для P4 CSS wiring (`scroll-timeline-name`, `view-timeline-name`, `animation-timeline`); 15 unit-тестов.
- 🟡 `lumen-paint` — display list (FillRect, **FillRoundedRect** (SDF-антиалиасинг: `CornerRadii { tl, tr, br, bl }` + WGSL `sdf_rrect` с `smoothstep`; `DrawBorder` расширен округлыми углами через tessellated quarter-annulus арки), **DrawBorder**, DrawText, **DrawOutline** (CSS Basic UI L4 §5: outline снаружи box-а; 4 fill-quad-а top/right/bottom/left; OutlineColor::Auto/CurrentColor резолвится в style.color; Phase 0 — все стили рисуются как Solid), **DrawImage**) + wgpu-растеризатор с двумя pipeline-ами (fill + text), **multi-size + variation-aware glyph atlas 1024×1024** (SIZE_BINS = [8,12,16,20,24,32,48,64]; ключ `AtlasKey { face_id, glyph_id, size_bin, coords_hash: u64 }`; растеризация на bin-подобранном размере без блюра при совпадении font-size с bin-ом; `coords_hash` от normalized variation coords позволяет VF variant-глифу не перезаписывать base-instance), текстурированные квады из atlas-а. `DrawBorder` рендерится `emit_border_side` per-edge: `Solid`/`None` = full-rect; `Dashed` = pattern `(2w, w)`; `Dotted` = `(w, w)`; **`Double` = два параллельных rect ~1/3 ширины с gap ~1/3 (fallback Solid при w<3px)**. `BorderStyle` расширен вариантом `Double`. Цвет с currentColor fallback. Под/над/перечёркивающие линии text-decoration эмитятся как FillRect-ы у baseline каждого фрагмента. **CSS Text Decoration L3 §6 `text-shadow`**: per-fragment DrawText-копии со смещением `(offset_x, offset_y)` и цветом тени (None → currentColor) эмитятся ДО основного DrawText в обратном порядке к CSS-списку (painter's order: first shadow on top). Phase 0 без blur (требует off-screen Gaussian pass). **CSS Backgrounds L3 §4.6 `box-shadow`** (outset + inset): outset = FillRect перед background со смещением + spread; inset (`emit_inset_box_shadows`, §3.5.1 «above background, below border») = до 4 FillRect-рамок между padding-box (outer) и `outer translated by offset + inset by spread` (inner), нулевые рамки skip-ятся, no-overlap → один outer FillRect, full-cover → skip. Multiple — reverse iter (first on top); color None → currentColor; transparent — skip; blur игнорируется (off-screen Gaussian pass — P2 п.4+); border-radius на тенях не учитывается. **CSS Backgrounds L3 §3.8 `background-clip`**: rect для background-FillRect определяется по `background_clip` — BorderBox (default) / PaddingBox (минус border) / ContentBox (минус border+padding). `Text` Phase 0 fallback на BorderBox (нужен glyph-mask + alpha-pass). **CSS Display L3 §4 `visibility: hidden`** в paint: Block/Image self не эмитим (bg/border/outline/shadow/image); InlineRun per-frag skip. Children обходятся (inherited visibility, child может вернуть себя через `:visible`). `Collapse` вне table-row эквивалентен Hidden. **CSS Color L3 §3.2 `opacity: 0`** Phase 0 skip: early-return в walk + emit_box_self — весь subtree (включая children) не paint. `opacity > 0 && < 1` пока без эффекта (требует off-screen compositor pass). `FontMeasurer` для TextMeasurer. **`lumen-paint::hit_test` (P2 п.2B)** — `hit_test(point, &LayoutBox) -> Option<HitTestResult { node, local_point, path }>`, обратный CSS Painting Order traversal (positive-z SC desc / in-flow + auto-0-z reverse-DOM / negative-z SC desc), фильтры `pointer-events: none` / `display: none`, transform inversion через `Mat4::invert_2d_affine()` (сингулярные forward-матрицы → бокс unhittable). Внешние зависимости: `wgpu` (exception #2), `winit` (exception #1)
- 🟡 `lumen-font` — собственный TrueType-парсер (head/maxp/cmap format 4+12/hhea/hmtx/loca/glyf/**fvar**/**avar**/**HVAR**/**VVAR**/**MVAR**/**gvar**) + scanline-растеризатор (квадратичные Безье, 4×4 AA, even-odd fill). cmap format 12 — Sequential Groups, полный Unicode U+10FFFF (эмодзи U+1F600+, SMP). **`fvar` parser (Variable Fonts L1 enabler)** — `Font::fvar() → Fvar { axes: Vec<VariationAxis { tag, min, default, max, flags, name_id }>, instances: Vec<NamedInstance { subfamily_name_id, flags, coordinates, post_script_name_id }> }`; named instances («Regular» / «Bold» / «Light Italic») — фиксированные точки в пространстве осей для UI font picker-а; `Fvar::instance_by_name_id(id)` lookup. **`avar` parser (axis normalization)** — `Font::avar() → Avar { segments: Vec<SegmentMap { maps: Vec<AxisValueMap { from, to }> }> }`, `Avar::normalize(axis_index, coord)` применяет piecewise-linear перенормализацию. **Composite glyphs ARGS_ARE_XY_VALUES=0 (point alignment)** — `Anchor` enum (`Offset(dx, dy)` / `Points { parent, child }`); `glyph_resolved` в point-mode вычисляет смещение как `parent.point[args1] − transformed_child.point[args2]` (рудиментарное TrueType-выравнивание pre-1996). **`ItemVariationStore` parser** — общий enabler для HVAR/MVAR/gvar: `VariationRegionList` (tent-функции на осях через F2DOT14 start/peak/end) + `ItemVariationData` blocks (region indexes + per-item delta_sets со смешанным i16/i8 storage). Format 1 only. **`DeltaSetIndexMap` parser** — HVAR/VVAR/MVAR glyph_id → (outer, inner) lookup: format 0 (16-bit map_count) и format 1 (32-bit), entry packed в 1..4 байта с настраиваемой inner-bit-разделкой. Per-spec out-of-range index → последняя entry. **`HVAR` parser** — `Font::hvar() → Hvar { store, advance_width_map, lsb_map, rsb_map }`, `advance_width_index(glyph_id)` через map или identity fallback (outer=0, inner=glyph_id) per spec. **`VVAR` parser** — `Font::vvar() → Vvar { store, advance_height_map, tsb_map, bsb_map, v_org_map }` — зеркало HVAR для вертикальных метрик (CJK vertical, Mongolian); identity fallback по advance_height, для TSB/BSB/vOrg отсутствующая map = «нет вариаций» (caller проверяет `has_*_variations()`). **`MVAR` parser** — `Font::mvar() → Mvar { store, records: Vec<ValueRecord { tag, delta_set_outer, delta_set_inner }> }`, `Mvar::lookup(tag)` O(log n) bin-search для standard metric tags (`xhgt`/`cpht`/`undo`/`unds`/`strs`/sub-super-script/ascender/descender). `ItemVariationStore::evaluate(outer, inner, coords)` ✅ (tent-function runtime: per-axis scalar + product по region-у, sum по data block). **`gvar` parser** ✅ — `Font::gvar() → Gvar<'a> { axis_count, shared_tuples, glyph_count, flags, glyph_data, glyph_offsets }` (lazy per-glyph: `glyph_variation_data(glyph_id) -> Option<&[u8]>`, `parse_glyph(glyph_id) -> Result<Option<GlyphVariationData>>`); поддержка short/long offsets через `flags & FLAG_LONG_OFFSETS`. `GlyphVariationData { tuple_variations: Vec<TupleVariation { peak, intermediate: Option<(start, end)>, points: PointNumbers::{All|Explicit(Vec<u16>)}, x_deltas: Vec<i16>, y_deltas: Vec<i16> }> }`. Реализованы packed point numbers (count 1/2-byte, runs с word/byte deltas, cumulative), packed deltas (zero / word / byte run-encoding), embedded peak / shared peak tuple lookup, intermediate region, private/shared/all point modes. Runtime `tuple_axis_scalar(coord, peak, intermediate)` + `tuple_scalar(coords, &variation)` — tent-функция с дефолтным region-ом из peak. **Variable fonts runtime готов (IUP + apply_glyph_deltas + Font::glyph_resolved_with_coords)** — `lumen-font::variation::apply_variations_to_simple_outline` применяет gvar deltas к outline-контурам с per-axis per-contour IUP (Interpolation of Untouched Points): untouched-точка между двумя touched-соседями clamp-ится к min/max orig либо linear interpolate, single-touched распространяется на весь контур, 0-touched — variation не вкладывает; phantom-points пропускаются (HVAR/VVAR обрабатывают отдельно). **`Font::glyph_resolved_with_coords(glyph_id, coords)`** — variable-fonts entry поверх resolve-pipeline: для simple-outline glyph применяет deltas через `apply_variations_to_simple_outline`; для composite — рекурсия с теми же coords к каждому child (gvar парсится один раз перед спуском и шарится через `Option<&Gvar>`). Пустой `coords` или font без `gvar` — short-circuit на путь `glyph_resolved`. `glyph_resolved` теперь тонкая обёртка над общим `glyph_resolved_inner(id, &[], None, 0)`. **Phase 0 ограничение:** component-level gvar variations (deltas на `CompositeComponent.anchor`) пока не применяются — отложено. 281 unit + 12 integration на Inter + 6 integration на synthetic-TTF с gvar. Осталось: CSS `font-variation-settings` cascade в lumen-layout, вызов из rasterizer, расширение glyph-atlas cache key normalized coords. Отложено: hinting, GSUB/GPOS shaping, CFF outlines, color glyphs, composite flags USE_MY_METRICS / OVERLAP_COMPOUND / SCALED_COMPONENT_OFFSET.
- 🟡 `lumen-encoding` — детектор кодировок и декодеры: **UTF-8, UTF-16 LE/BE, Windows-1251, KOI8-R, CP866**. Пайплайн: BOM (UTF-8/UTF-16 LE/UTF-16 BE) → `<meta charset>`-sniff (1 КБ) → HTTP content-type hint → UTF-8 валидность → частотная эвристика по русским буквам. UTF-16 декодер обрабатывает surrogate-пары (BMP + supplementary через U+10000+), lone surrogates и нечётное число байт → U+FFFD. Реализует `EncodingDetector` из `lumen-core::ext`. 59 тестов (включая UTF-16 surrogate-пары, emoji, ASCII/cyrillic в обоих endian). Отложено: ISO-8859-5, MacCyrillic, prescan по HTML5 spec §12.2.3.2 (точные правила парсинга атрибутов)
- 🟡 `lumen-image` — собственный декодер растровой графики. **PNG-декодер** для Gray / GrayAlpha / RGB / RGBA при `bit_depth ∈ {8, 16}` (16-bit downsample-ится до 8 бит на канал отбрасыванием младшего байта — эквивалент `PNG_TRANSFORM_STRIP_16` в libpng) + **palette (color_type 3) с опц. tRNS + 1/2/4-bit grayscale и palette** (sub-byte unpack + scaling по PNG §13.12) + **tRNS для non-palette grayscale/RGB (single-color transparency, Gray8→GrayAlpha8 / Rgb8→Rgba8 с бинарным match-ом)** + **Adam7-interlacing для всех color types** (decode_adam7 — 7 passes раскладываются в финальный row-major буфер): свой CRC32 (IEEE 802.3 reflected), chunk reader, IHDR + PLTE + tRNS parsers, DEFLATE/inflate (RFC 1951: stored/fixed/dynamic Huffman, LZ77 окно 32 КБ), zlib-обёртка (RFC 1950 + adler-32), развёртка фильтров скан-линий (None/Sub/Up/Average/Paeth), bit-unpacking MSB-first, grayscale-масштабирование (1-bit×255, 2-bit×85, 4-bit×17), расширение палитровых индексов в Rgb8 / Rgba8. **JPEG baseline (SOF0) + progressive (SOF2)** декодер (ISO/IEC 10918-1): 8-bit precision, Y-only grayscale → Gray8 и 3-component YCbCr → Rgb8 через ITU-R BT.601 (JFIF §7); chroma subsampling 4:4:4 / 4:2:2 / 4:2:0 с nearest-neighbour upsampling; canonical Huffman (Annex C) build с Kraft-McMillan валидацией; bit reader с byte-stuffing `FF 00 → FF` и остановкой на маркерах; прямой 2D IDCT 8×8 в фиксированной точке ×1024; restart intervals (DRI + RST0..RST7 с циклическим счётчиком и сбросом DC predictors); marker reader для SOI/EOI/SOF0/SOF2/DHT/DQT/SOS/DRI/APPn/COM. **Progressive multi-scan loop** (§G): coefficient buffers per-component, 4 типа scan — DC initial (`<< Al`), DC refinement (1 бит → позиция Al), AC initial (RLE+EOBn extension `<< Al`), AC refinement (§G.1.2.3 — refine non-zero 1 битом, новые non-zero вставляются между ними; ZRL пропускает 16 zero-positions; EOBn вводит EOB-run mode); переопределение Huffman-таблиц между scan-ами через `read_next_segment_after_scan`; финализация (dequantize + IDCT + upsample) после EOI. Прочие SOFn (extended/lossless/hierarchical/arithmetic) и DAC отвергаются. Никаких сторонних crate-ов (см. §5). 130 unit + 57 integration тестов на реальных PNG/JPEG-фикстурах (включая progressive 4:4:4 / 4:2:0 / grayscale + gradient-ы + jpegtran-сгенерированный полный DC-refinement scan-script). Отложено: 12-bit / CMYK / ICC из APP2, WebP, AVIF — отдельными задачами.
- ✅ `lumen-network` — HTTP/1.1 + HTTPS клиент (rustls, exception #3). Redirect, chunked TE с **корректным дочитыванием trailer-секции** (без этого keep-alive ломался — хвост старого ответа попадал в следующий status-line). URL берётся из `lumen_core::url::Url` — никакого собственного парсинга здесь нет; **IDN-домены** конвертируются в Punycode через `Url::host_ascii()` непосредственно перед TCP/TLS/Host-header (DNS/TLS SNI/Host получают `xn--…` форму). `HttpClient` реализует `NetworkTransport`. **EventSink-интеграция** (принцип №4 «каждый исходящий байт виден»): `HttpClient::with_sink/with_tab` builder, эмит `RequestStarted` перед сокетом и `RequestCompleted` после получения статуса — для каждого редирект-хопа отдельная пара событий. Редирект-Location резолвится через `Url::resolve` (RFC 3986 §5.3). **RequestFilter hook** (`with_filter`): per-URL `should_block` проверяется до RequestStarted и до TCP; на блок эмитится `RequestBlocked { reason }` (а не Started/Completed) и `fetch` возвращает Err. **HTTP/1.1 keep-alive + ConnectionPool** (`HttpClient::with_pool` или собственный по умолчанию): `Connection: keep-alive` в request-header, после успешного ответа TCP/TLS-соединение возвращается в Mutex<HashMap<(host,port,is_tls), Vec<Entry>>> с timestamp; следующий запрос к тому же origin переиспользует idle (LIFO, idle_timeout=30 с, max_idle_per_host=6); `Connection: close` от сервера, EOF или ошибка чтения трактуются как `closed` и не идут в пул; **retry-on-stale** — при попадании на закрытое сервером idle-соединение клиент один раз перезапускает запрос на свежем connect-е (детектится по io::ErrorKind::BrokenPipe/ConnectionReset/UnexpectedEof + наши EOF-сообщения). **DnsResolver hook** (`with_dns_resolver`): resolve вынесен в trait-точку из `lumen-core::ext`, default = `SystemDnsResolver` (через `(host, port).to_socket_addrs()`), подменяется на `CachedDnsResolver` (lumen-storage) для TTL-кеша или на `DohResolver` (см. ниже); connect двухэтапный — `resolver.resolve()` → try-each `SocketAddr` до первого успешного `TcpStream::connect`. Per redirect-hop вызывается независимо. **DoT resolver** (`lumen_network::DotResolver`, RFC 7858): реализует `DnsResolver` поверх собственного TCP+TLS-сокета (rustls, exception #3, без HTTP). Конструктор `DotResolver::new(server_name, server_addr)` принимает pre-resolved IP+порт DoT-сервера (bootstrap-разделение от system DNS); фабрики `cloudflare()` / `google()` / `quad9()` зашивают hardcoded IP-литералы `1.1.1.1` / `8.8.8.8` / `9.9.9.9` на порт 853 (`DOT_DEFAULT_PORT`). На каждый `resolve` шлёт AAAA+A последовательно, IPv6 prefer (RFC 6724 §6), свежий TLS handshake per query (Phase 0; persistent — отложено). Wire-format переиспользует `doh::encode_query` / `doh::decode_answer_ips` (теперь `pub`); сверху — собственный TCP framing (RFC 1035 §4.2.2 `[u16 BE length][message]`) через `frame_query` / `read_framed_message`. `query_over_stream<S: Read+Write>` — generic exchange-функция, тестируется через mock `Cursor<Vec<u8>>` без поднятия TLS. IP-литералы bypass. **DoH resolver** (`lumen_network::DohResolver`, RFC 8484): реализует `DnsResolver` поверх произвольного `NetworkTransport` (типично — собственный `HttpClient` с bootstrap-резолвером); на каждый `resolve` шлёт два GET (AAAA + A, объединяет с IPv6 prefer по RFC 6724 §6) с `?dns=<base64url(query)>` к DoH endpoint-у; собственный DNS wire-format encoder/decoder (RFC 1035 §4 — header/flags/labels/compression pointers §4.1.4, A/AAAA RDATA, CNAME пропускается); собственный base64url без padding (RFC 4648 §5); IP-литералы (`8.8.8.8`, `::1`, `[::1]`) bypass без обращения к endpoint-у; RCODE≠0 / TC=1 → Err. **HSTS enforcement** (`with_hsts`, RFC 6797): trait-точка `HstsEnforcement` (lumen-core::ext), реализация — `lumen-storage::hsts::HstsStore`, fail-open. Pre-request §8.3 — http→https upgrade для known-hosts с правильной port-mapping (явный :80 убирается, custom-port сохраняется); upgrade ДО RequestFilter/RequestStarted — observer и блок-листы видят upgraded URL. Post-response §8.1 — парсинг `Strict-Transport-Security` только из HTTPS-ответов (HTTP STS игнорируется как небезопасный), `max-age=0` сохраняется как «снять HSTS». Каждый redirect-hop проверяется независимо. **HTTP Range requests** (RFC 7233, `fetch_range(url, RangeSpec, Option<RangeValidator>) -> Result<RangeResponse>`): single-range запросы в трёх формах — closed `bytes=START-END`, open-ended `bytes=START-`, suffix `bytes=-N` (последние N байт); опциональный `If-Range` validator (ETag / Last-Modified, дословно копируется в header). 206 ответ парсит `Content-Range` в типизированный `ContentRange {start, end, total: Option}`, 200 фолбэк (server игнорирует Range или If-Range mismatch — ресурс изменился) даёт full body + content_range=None, 416/4xx/5xx → Err. Range + If-Range пробрасываются в redirect-target. **Multi-range** (RFC 7233 §3.1 + §4.1 + §A, `fetch_multi_range(url, &[RangeSpec], Option<RangeValidator>) -> Result<MultiRangeResponse>`): один запрос на несколько диапазонов через `Range: bytes=0-99,200-299,1000-`. Внутри — типизированный `RangeRequest::{Single(RangeSpec) | Multi(Vec<RangeSpec>)}` (общий тип для single и multi путей, internal-only); невалидные spec-ы внутри Multi молча отбрасываются, пустой/всё-невалидный набор → Err до открытия сокета. Сервер ответил тремя способами: (а) `200 OK` (Range проигнорирован) → один `RangePart { body=full, content_range=None }`; (б) `206` с обычным `Content-Range` (сервер слил соседние диапазоны) → один RangePart с распарсенным Content-Range; (в) `206` с `Content-Type: multipart/byteranges; boundary=X` → `parse_multipart_byteranges` режет body на parts (каждый — собственный header-set + Content-Range + body). `parse_boundary_from_content_type` принимает quoted/unquoted boundary, case-insensitive type/param, любой `multipart/*` (не только byteranges). Парсер multipart лояльный: преамбула/эпилог игнорируются (RFC 2046 §5.1.1), отсутствие Content-Range в part-е оставляет `content_range=None`, CRLF/LF в headers оба принимаются, бинарные body с embedded `\r\n` работают (boundary — уникальный токен). 416/4xx/5xx → Err. `<video>` seek по таблице сегментов, PDF page-load, resume downloads с несколькими «дырами» — на этом enabler-е. **HTTP auth — RFC 7235 + 7617 Basic + 7616 Digest** (`with_credentials(Arc<dyn HttpCredentialProvider>)`): на 401 + `WWW-Authenticate` парсим challenge-list (token / quoted-string с `\` escape, корректное различение «,» как separator challenges vs auth-param), выбираем strongest (Digest+SHA-256 > Digest+MD5 > Basic), формируем `HttpAuthChallenge { origin, realm, scheme }`, опрашиваем `HttpCredentialProvider`, собираем `Authorization`: Basic = `base64(user:pass)`, Digest = HA1 / HA2 / response по MD5 / MD5-sess / SHA-256 / SHA-256-sess (qop=auth + RFC 2069 legacy без qop); собственные MD5 (RFC 1321) + SHA-256 (FIPS 180-4) — не security-критично (Digest = challenge-response, не KDF). Один retry на hop, Authorization не пересылается через 3xx (RFC 7235 §3.1). `StaticCredentialProvider` для тестов / `-u user:pass`. 194 тестов (+38 DoH: 8 encode + 11 decode + 5 base64url + 10 resolver integration через mock-NetworkTransport + 4 service; +22 DoT: 4 frame_query + 5 read_framed_message + 5 query_over_stream через mock Read+Write + 3 IP-литерал bypass + 5 service; +49 HTTP auth: 8 hash vectors RFC 1321/FIPS 180-4 + 2 base64-std + 9 parser + 4 select + 8 builder + 11 mock-server retry; +21 Content-Encoding: 8 BrotliContentDecoder unit + 8 apply_content_encoding unit + 2 builder + 3 e2e через mock-TcpListener). **Content-Encoding pipeline**: `HttpClient::with_content_decoder(Arc<dyn ContentDecoder>)` регистрирует декодер; `Accept-Encoding` запроса собирается из имён (порядок регистрации = порядок предпочтения); `Content-Encoding` ответа парсится (case-insensitive, comma-separated), `identity`/пустые токены пропускаются, encoding без декодера → Err; для stacked encodings декодеры применяются в обратном к header-у порядке (RFC 7231 §3.1.2.2). **`BrotliContentDecoder`** (`crates/network/src/brotli.rs`) — реализация `ContentDecoder` (encoding `"br"`) поверх **provisional `brotli-decompressor` = "5"** (RFC 7932, §5 «Provisional accelerators»), trait-anchor — `ContentDecoder` в `lumen-core::ext`, graduation criterion — реалистично никогда (формат стабилен с 2016). **Mixed-content enforcement** (W3C Mixed Content §5, P3 2A шаг b): `MixedContentPolicy { top_level: Origin, mode: MixedContentMode { Disabled | SpecDefault | Strict } }` + builder `with_mixed_content_policy(top_level, mode)` + публичный `fetch_subresource(url, destination) -> Result<Vec<u8>>`. Classify+block в `fetch_with_redirect` ПОСЛЕ HSTS upgrade и ДО `RequestFilter` / `RequestStarted`: blockable mixed-content (scripts / styles / iframes / fonts / fetch / worker) в SpecDefault и в Strict; OptionallyBlockable (images / media / prefetch) — только в Strict. Event::RequestBlocked с reason формата `"mixed-content: blockable" | "mixed-content: optionally-blockable"`, per redirect-hop. `NetworkTransport::fetch(url)` (top-level navigation) — без destination и enforcement. **CORS preflight enforcement** (Fetch §3.2.2 — §4.10, P3 2A continuation finish): `with_cors_cache(Arc<PreflightCache>)` + `fetch_cors(CorsRequest, Option<RequestDestination>)`. Hop-локальная классификация cross-origin/same-origin внутри `fetch_with_redirect`: cache.allows shortcut → если miss и needs_preflight — OPTIONS preflight через тот же `fetch_single` (с pre-formatted Origin/ACRM/ACRH extra-headers), эмит Started+Completed для preflight-байт; evaluate → success cache.insert, fail RequestBlocked `cors-preflight: <CorsError>`. Actual cross-origin: method = cors.method, extra = `Origin: <serialize>` + author-headers с фильтрацией дубликатов наших Host/Connection/UA/Accept/Accept-Encoding/Authorization/Range/If-Range/Content-Length/Origin. Actual response: `check_cors_response_headers` per cross-origin hop ДО status-branching; fail → RequestBlocked `cors-response: <CorsError>`. Redirect: тот же `cors_ctx` прокидывается, каждый hop re-classify-ится под `Origin::from_url(next)`. Phase 0 ограничения: HttpClient GET-only (POST/PUT/PATCH без body), cookie-jar не интегрирован (credentials_mode влияет только на ACAO=`*` rejection + ACAC requirement). +8 интеграционных тестов через mock-TcpListener. **CORS preflight classifier + cache** (Fetch §3.2.2 — §4.10): pure-логика в `lumen-network::cors`. `is_cors_safelisted_method` (GET/HEAD/POST), `is_forbidden_request_header` (exact + `sec-`/`proxy-` префиксы), `is_cors_safelisted_request_header` (Accept/Accept-Language/Content-Language/Content-Type/Range с essence-парсингом и 128-байт-лимитом). `CorsRequest { origin, target, method, headers, credentials_mode }`, `CredentialsMode { Omit, SameOrigin (default), Include }`. `needs_preflight(req)` (method не safelisted ∨ хоть один header не safelisted). `unsafe_request_header_names` — lowercased, deduplicated, sorted (Fetch §4.8 step 7.1). `build_preflight_headers` — `Origin`/`Access-Control-Request-Method`/`Access-Control-Request-Headers`. `PreflightResult { allowed_methods, allowed_headers, allow_credentials, max_age_seconds }` + `method_allowed` (`*` wildcard + safelisted-implicit) + `unmatched_header` (`*` wildcard кроме `Authorization`). `evaluate_preflight_response(status, headers, req)` — status 200-299, ACAO (validate exact-match через `Origin::serialize` или `*`-если-не-Include), ACAC (`true` обязателен при Include), ACAM, ACAH, ACMaxAge (default 5 сек, invalid → Err). Финальная проверка actual method+headers против allow-lists. `check_cors_response_headers(headers, origin, mode)` — отдельный entry для actual response (ACAO+ACAC, без ACAM/ACAH). `PreflightCache` — thread-safe (`Mutex<HashMap>`), ключ `(requestor_origin, target_origin, credentials_mode)`, TTL = `max_age_seconds`, lazy-expire на lookup, `allows_at(&req, now)` shortcut. 56 unit-тестов. Этот модуль — **классификатор и спецификация, не enforcer**; реальная отправка OPTIONS + cache-hooks в `HttpClient::fetch_with_redirect` — следующая задача (по аналогии с mixed-content split: classifier → enforcement). **HTTP response cache (RFC 7234)** (`crates/network/src/http_cache.rs`): `HttpCache` — thread-safe in-memory store (`Mutex<HashMap<url, CacheEntry>>`); `HttpClient::with_http_cache(Arc<HttpCache>)` builder; интеграция в `NetworkTransport::fetch` и `fetch_subresource` — cache-key = URL без фрагмента; `CacheControl::parse` (max-age, s-maxage, no-store, no-cache, must-revalidate); `CacheEntry::is_fresh()` — проверяет `Instant::now() < expires_at`; heuristic freshness RFC 7234 §4.2.2 = 10% от (Date − Last-Modified); `CacheEntrySnapshot::conditional_headers()` — генерирует `If-None-Match` (ETag) или `If-Modified-Since` (Last-Modified); `fetch_with_redirect` принимает `cache_extra_headers: &str` — добавляются к запросу, сбрасываются на redirect; 304 Not Modified — явный match arm, возвращает `Ok(resp)`, вызывает `cache.revalidate()`; `revalidate` обновляет ETag и Last-Modified из 304-заголовков. 543 тестов (+19 http-cache: 14 unit в http_cache.rs + 5 интеграционных через mock-TcpListener в lib.rs).
- ✅ `lumen-storage` — два бэкенда `StorageBackend`: in-memory KV с snapshot LUMEN_KV_V1 (для тестов / ephemeral) + **SqliteStorage** (persistent, через `rusqlite` bundled — exception #5; WAL + synchronous=NORMAL; одна таблица `kv` с composite PK). Полное origin-партиционирование в обоих. **CookieJar** — RFC 6265 / RFC 6265bis cookies поверх SQLite: domain/path matching, expires_at TTL, secure-only-HTTPS, SameSite (Strict/Lax/None), top_level_site partitioning для total cookie protection (§9.2); `parse_set_cookie_with_psl` применяет RFC 6265bis §5.5 step 5 — public-suffix защита Domain attribute (super-cookie reject + host-only fallback). **History** — посещённые страницы (url/title/visit_date/visit_count/favicon_hash/text_sha256) с upsert-semantics и API recent/most_visited — основа под §12.1 полнотекстовый поиск. **CachedDnsResolver** (`lumen-storage::cached_dns`) — реализация `DnsResolver` поверх `DnsCache`: оборачивает произвольный inner-resolver (system / DoH в будущем), на каждый `resolve` сначала пытается hit по кэшу (с TTL и порт-подстановкой на каждый вызов — порт не кэшируется), при miss идёт в inner и `cache.put` с `default_ttl_seconds`. `Clock` trait для подмены времени в тестах. **SafeBrowsingList** (`lumen-storage::safe_browsing`) — локальный аналог Google Safe Browsing v4 без облачного API (принцип №1): таблица `safe_browsing(list_name, full_hash BLOB(32), threat_type, added_at)` с composite PK + index по full_hash; ThreatType { Malware / SocialEngineering / UnwantedSoftware / PotentiallyHarmful / Other(_) }; canonicalize URL + 5 host-suffix × 4 path-trim вариантов на запрос; **`SafeBrowsingFilter::with_psl`** — host-suffix enumeration обрезается до eTLD+1 (через `PublicSuffixList`), что блокирует ложно-широкие матчи через shadow-entry на public suffix; `SafeBrowsingFilter` реализует `RequestFilter`, fail-open на ошибки lookup. **`PslProvider`** (`lumen-storage::psl`) — реализация `PublicSuffixList` через provisional **`psl = "2"`** (compiled-in таблица, codegen из public_suffix_list.dat на этапе сборки). **`IdbBackend` trait + `IdbStore`** (`lumen-core::ext` + `lumen-storage::indexed_db`) — Rust-бэкенд для IndexedDB JS-шима: сериализует все базы origin в tagged-JSON снимок, `_lumen_idb_persist` после каждого мутирующего flush, `_lumen_idb_load` при init → базы переживают reload. 442 тестов.
- 🟡 `lumen-knowledge` (§12) — базовая FTS5-таблица `history_fts(url, title, text)` поверх SQLite с tokenizer `unicode61` и bm25-ранжированием готова. **§12.2 заметки** (`Notes` с external content FTS5 `notes_fts(selection, comment)` и triggers для авто-sync) и **§12.3 read-later** (`ReadLater` с html_snapshot BLOB, status, tags + external content FTS5 + триггеры) готовы. API: index/unindex/search для HistoryFts; add/update/delete/list_for_url/recent/search для Notes; save/set_status/touch/get/list_by_status/search для ReadLater. 39 тестов. Отложено: §12.4 поиск по открытым вкладкам, §12.2 Range API для highlight-наложений, Porter-stemmer для русского, §12.3 фоновый downloader для ресурсов при save.
- ⬜ `lumen-ai` (§12.5) — опциональный, embedding + RAG поверх локального LLM. Phase 3+, feature-flag

### Политика зависимостей (§5, обновлена 2026-05-15)
- ✅ Зафиксирована (две категории, см. §5): **Permanent exceptions** — никогда не пишем сами; **Provisional accelerators** — берём готовое сейчас, заменяем по событию. Ядро (HTML/CSS/DOM/layout/paint/font/encoding/URL/HTTP/1.1+2/DNS/adblock/knowledge/UI) — всегда наше.
- ✅ Permanent #1: `winit` (OS event loop) — за `WindowingBackend`
- ✅ Permanent #2: `wgpu` (GPU API) — за `RenderBackend` — активирован в `lumen-paint`
- ✅ Permanent #3: `rustls` + `webpki-roots` (TLS / crypto + Mozilla CA bundle) — за `TlsBackend` — активирован в `lumen-network`
- ✅ Permanent #4: SQLite (`rusqlite` с `bundled`) — за `StorageBackend` + `KnowledgeStore` — активирован в `lumen-storage` и `lumen-knowledge`
- ✅ Permanent #5: JS engine (`rquickjs` v0.11 → `rusty_v8` v1.0+) — за `JsRuntime` — активирован в `lumen-js` (Phase 0: eval/globals/call; shell feature `quickjs`; 2026-05-20)
- 🟡 Provisional (3 подключено: `brotli-decompressor` в `lumen-network` через `BrotliContentDecoder` за `ContentDecoder`; **`psl`** в `lumen-storage` через `PslProvider` за `PublicSuffixList` — RFC 6265bis §5.5 cookies + Safe Browsing host-suffix; **`hyphenation`** в `lumen-encoding` через `KnuthLiangHyphenation` за `HyphenationProvider` — CSS `hyphens: auto`, 11 локалей). Ожидают подключения: image decoders (JPEG/WebP/GIF), `icu4x`, `ruzstd`, `idna`, `woff2`, `hunspell-rs`/`spellbook`, `quinn`. Каждый — за trait в `lumen-core::ext`, подключается по мере того, как фаза реально упирается в задачу. Полная таблица + graduation criteria — в §5.

### Точки расширения (trait-ы из `lumen-core::ext`)
- ✅ `StorageBackend` — две реализации в `lumen-storage`: `InMemoryStorage` (ephemeral, snapshot LUMEN_KV_V1) + `SqliteStorage` (persistent, через rusqlite/bundled — permanent #4). 30 тестов.
- ✅ `NetworkTransport` — реализован в `lumen-network::HttpClient` (HTTP/1.1 + HTTPS через rustls, redirect, chunked, 12 тестов)
- 🟡 Интерфейсы: `SearchProvider`, `FilterListSource`, `RequestFilter` — определены; `RequestFilter` уже интегрирован в `HttpClient::with_filter` (hook готов, реализации фильтров нет)
- 🟡 **Sprint 0 P3 trait-anchors** (см. описание `lumen-core` выше): `UnicodeProvider`, `IdnaProvider`, `PublicSuffixList`, `ContentDecoder`, `FontFormat`, `SpellChecker`, `HyphenationProvider`, `JsRuntime` — все 8 определены, Null-stub-ы тестируются на dyn-safety и «не поддерживается». Реальные реализации — по мере необходимости через provisional-крейты §5
- ✅ `HstsEnforcement` — реализация `lumen-storage::hsts::HstsStore` (impl-блок поверх существующего SQLite-store), потребитель `lumen-network::HttpClient::with_hsts(...)` — RFC 6797 end-to-end: pre-request http→https upgrade + post-response persist `Strict-Transport-Security` per-hop
- ✅ `HttpCredentialProvider` — trait `credentials(&HttpAuthChallenge) -> Option<HttpCredentials>` для HTTP Basic + Digest по `(origin, realm, scheme)`. Реализация `lumen-network::StaticCredentialProvider` (in-memory, fallback-chain) для тестов / curl-style; потребитель `HttpClient::with_credentials(...)` — RFC 7617 Basic + RFC 7616 Digest (MD5 / SHA-256 / qop=auth + legacy). UI-popup и keyring — следующие задачи P3 (бывший P4 / платформенный слой)
- ✅ `EncodingDetector` — реализован в `lumen-encoding::HeuristicDetector` (BOM + meta + content-type + UTF-8 + частотная эвристика)
- ⬜ Trait-ы для 4 exceptions: `WindowingBackend`, `RenderBackend`, `TlsBackend`, `JsRuntime` — задокументированы как future в `lumen-core::ext`, code-уровень добавим при первом использовании
- ⬜ `KnowledgeStore` (§12) — FTS / read-later / notes. Phase 2
- ⬜ `AiBackend` (§12.5) — embed / generate, опционально. Phase 3+
- ✅ **`MemoryPressureSource`** (ADR-008, task 10H) — `MemoryPressureLevel` + trait + `NullMemoryPressureSource` в `core/src/ext.rs`; Win32 (`GlobalMemoryStatusEx`) + Linux PSI (`/proc/pressure/memory`) + macOS (`host_statistics64(HOST_VM_INFO64)`) реализации в `core/src/memory_pressure.rs`; `on_memory_pressure(level)` в `ImageDecodeCache` / `GlyphAtlas` / `LayerCache`. Shell integration ⬜.
- ✅ **`JsRuntime` расширение** (ADR-008, task 10C) — `pause()` / `unpause()` / `suspend() -> SuspendedHeap` / `resume(SuspendedHeap)` реализованы в `rquickjs` через `JS_WriteObject`/`JS_ReadObject`. V8 в Phase 3 — отдельная задача.

### Уникальные фичи (§12) — план на Phase 1-4
- ⬜ **Automation API first-class (§6.11, ADR-006)** — `BrowserSession` trait в `lumen-driver` + три транспорта (in-process / MCP / WebDriver BiDi); a11y-tree как primary locator; native input; auto-wait внутри движка; deterministic mode. **Phase 0**: trait + in-process для собственных `graphic_tests/`. **Phase 1**: MCP-сервер для AI-агентов. **Phase 2**: BiDi-сервер для Playwright/Selenium/Cypress.
- ⬜ **Anti-detection privacy stack (§9.5, ADR-007)** — 6-слойный privacy-stack по умолчанию: surface API без automation-маркеров, TLS JA3 как у Chrome, HTTP/HTTP2 layer matching Chrome, Brave-style rendering fp, opt-in behavioral mimicry для automation тестов, профили Standard/Strict/Tor. Цель: обычный юзер с Lumen не помечен как бот на Cloudflare/DataDome/Akamai. **Red lines**: нет CAPTCHA-solver, нет built-in IP rotation, нет anti-fraud-bypass.
- ⬜ **Tab lifecycle: пятитайерная RAM-модель (§11.4, ADR-008)** — T0 Active / T1 Background-recent / T2 Background-old / T3 Hibernated / T4 Closed-recoverable; transitions по timer + OS memory pressure + LRU. **Цель**: 50 открытых вкладок в Lumen ~400 MB vs Chrome 6-10 GB. **Структурные инварианты**: DOM arena serializable (не Rc<RefCell>), JsRuntime suspend/resume, layout/paint pure functions. **Restore SLO**: T1→T0 ≤ 50ms, T2→T0 ≤ 200ms, T3→T0 ≤ 1.5s. Главный продуктовый дифференциатор по RAM наряду с приватностью.
- ⬜ Tab session export / import (§12.7) — Phase 1
- ✅ Полнотекстовый поиск по истории (§12.1) — FTS5 + bm25 + omnibox-интеграция: `@history` prefix, dropdown до 7 строк, ArrowUp/Down, SearchHistory record; Porter-stemmer (Phase 2)
- 🟡 Аннотации и заметки (§12.2) — `lumen-knowledge::Notes` storage layer готов; Range API для восстановления highlight-наложений на странице — отложено
- 🟡 Read-later / офлайн-чтение (§12.3) — `lumen-knowledge::ReadLater` storage layer готов (status, tags, FTS5); фоновый downloader для ресурсов при save и UI отложены
- ⬜ Поиск по содержимому открытых вкладок (§12.4) — Phase 2
- ⬜ Focus mode (§12.6) — Phase 2
- ⬜ Кастомизация UI (drag&drop, темы) (§12.10) — Phase 2-3
- ⬜ Локальный AI layer (§12.5) — Phase 3+, опционально
- ⬜ Семантические закладки (§12.8) — Phase 3, зависит от AI
- ⬜ Граф знаний (§12.9) — Phase 3+
- ⬜ Кросс-устройственная синхронизация E2E (§12.11) — Phase 4+, требует mobile
- ⬜ DevTools (инспектор / консоль / network) (§12.12) — Phase 4+, не уникально, но необходимо
- ⬜ Tab UX: вертикальные/tree-style вкладки, workspaces, split view, auto-archive (§12.13) — Phase 2
- ⬜ Power-user input: vim-keys, gestures, omnibox-алиасы, regex find (§12.14) — Phase 2-3
- ⬜ Privacy UX: встроенный блокировщик, per-site контролы, cookie-banner dismiss (§12.15) — Phase 2
- ⬜ Web platform baseline: Passkeys/WebAuthn, контейнеры, sidebar web panels (§12.16) — Phase 2-3

### Локализация / RU (§10)
- ✅ DOM держит кириллицу (UTF-8) — зафиксировано тестами
- ✅ `Url::parse` принимает кириллические домены (тест на `президент.рф`)
- ✅ Encoding detection (cp1251, KOI8-R, CP866) — крейт `lumen-encoding`, подключён в shell
- ⬜ Cyrillic font fallback в paint
- ✅ Punycode/IDN — `lumen_core::punycode` (RFC 3492 encode) + `lumen_core::idn::domain_to_ascii`; `Url::host_ascii()` отдаёт ASCII-форму host для DNS/TLS/Host header — единственная точка вызова `idn::domain_to_ascii` среди потребителей
- ⬜ Локаль ru-RU (дата/время/числа)
- ⬜ UI-переводы (Fluent)

### Следующие шаги
- ✅ HTML parser — полный набор named entities (2125 WHATWG), RAWTEXT/RCDATA, DOCTYPE/quirks, srcset/sizes, picture, preload scanner, push-tokenizer + incremental tree builder; 335 тестов
- ✅ CSS parser — все структурные/функциональные/UI-state/link/form-state pseudo, `:is`/`:where`/`:has`, `@media`, `@property`, custom properties, `!important`; 119 тестов
- ✅ Layout — block/inline/flex/grid/positioned layout, CSS Variables, math-функции, text-decoration, visibility, opacity, Shadow DOM, CSS Transitions, CSS Transforms; полный CSS cascade с specificity
- ✅ Paint — display list + wgpu-rasterizer + glyph atlas + text rendering
- ✅ Связка движка с UI: shell открывает `samples/page.html` с фонами и текстом
- ✅ lumen-image — PNG (8/16-bit + palette + tRNS + Adam7) + JPEG baseline (DCT/Huffman/YCbCr) + WebP (VP8 lossy + VP8L lossless, `image-webp`) декодеры; **GIF static + animated** (`decode_gif_animated` → `AnimatedGif { frames, width, height, loop_count }`, `AnimatedFrame { image, delay_cs }`, `frame_index_at(elapsed_ms)`, `frame_at(elapsed_ms)`; shell-handoff: P3 вызывает `frame_at(elapsed_ms)` на каждом тике); `ImageDecoder` trait в `lumen-core::ext`; `supported_mime_types()` для `<picture>` type-filter; AVIF — отдельной задачей
- ✅ Composite glyphs в lumen-font (Cyrillic 'А' и другие) — `Anchor` enum (Offset/Points), `glyph_resolved` point-alignment; `crates/engine/font/src/glyf.rs`
- ✅ Свой HTTP/1.1 + TLS через `rustls` — `lumen-network::HttpClient` (redirect, chunked, keep-alive, pool, DoH/DoT, HSTS, auth, CORS, HTTP/2); `crates/network/`
- ✅ **`lumen-driver` крейт + `BrowserSession` trait + `InProcessSession`** — `core/src/ext.rs:1514` + `driver/src/session.rs`; §6.11, [ADR-006](docs/decisions/ADR-006-automation-api.md). 8A.1–8A.6 завершены; `cpu_raster` покрывает все 57 html-страниц graphic_tests.
- ✅ **Tab lifecycle инварианты** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)): (1) DOM arena ✅ (10B — `NodeId(u32)` + `to_bytes`/`from_bytes` via bincode); (2) JsRuntime suspend/resume ✅ (10C — `pause/unpause/suspend/resume` в `rquickjs`); (3) layout+paint pure ✅ (10D.1/10D.2 audit). Все три инварианта закрыты.

---

## Roadmap — приоритизация задач

**Перерабатано 2026-05-15 под 3 программистов с акцентом на параллельность.** Главный приём — **interface-first**: сначала публикуются типы/трейты (с `todo!()` или stub), дальше каждый реализует *против stub-а*, не дожидаясь impl у других. Реальная стыковка происходит drop-in (пустой stub → реальный impl, потребитель ничего не правит).

Маркеры `[P4]` из истории не удаляются (commits, prior fundamentals); в новом roadmap-е используются только `[P1] / [P2] / [P3]` и комбинации `[P1+P2] / [P1+P3] / [P2+P3] / [P1+P2+P3]`. Бывший P4 поглощён P3 — таблица в `CLAUDE.md` отражает это.

---

### Sprint 0 — Контракты (1-2 дня, общий PR `contracts-3-devs`)

Каждый кладёт **только сигнатуры** (типы/трейты), `todo!()` в теле. Этот PR разблокирует параллельные треки ниже без каких-либо ожиданий.

- ✅ **`[P1]`** `StackingContextId`, `PaintOrder`, `PropertyTrees { transform, scroll, effect, clip }`, типизированные `Length` / `Color` (как уже есть в `lumen-layout::values`, дополнить полями ComputedStyle), trait `AnimationInterpolator` со stub-реализацией. **Реализовано в ветке `sprint0-p1-contracts`**: модули `lumen-layout::stacking` (StackingContextId / PaintPhase / PaintOrder / StackingTree), `property_trees` (4 дерева + Mat4 + PropertyTreeNodeId), `animation` (AnimValue + AnimationInterpolator trait + NoopInterpolator step-half stub). ComputedStyle уже типизирован (Color struct, BorderStyle enum, resolved-px f32) — типизация declaration-level (text-value → AST) остаётся отдельной задачей 1B.
- **`[P2]`** `trait Compositor { fn commit(&mut self, trees: &PropertyTrees, layers: &LayerTree); }`, `trait LayerTree`, расширения `DisplayCommand` для blend / clip / layer / image upload.
- ✅ **`[P3]`** Все trait-anchors из политики зависимостей: `UnicodeProvider`, `IdnaProvider`, `PublicSuffixList`, `ContentDecoder` (с расширением для `br` / `zstd`), `FontFormat` (под WOFF2), `JsRuntime` (под rquickjs/V8), `SpellChecker`, `HyphenationProvider`. Каждый — со stub-реализацией, возвращающей «не поддерживается» (`NullJsRuntime`, `NullUnicodeProvider`, `NullIdnaProvider`, `NullPublicSuffixList`, `UnsupportedContentDecoder`, `NullFontFormat`, `NullSpellChecker`, `NullHyphenationProvider`). **Это — главный enabler параллельности.** Готово в ветке `contracts-p3` (merge 2026-05-15).

После merge `contracts-3-devs` стартуют три независимых трека.

---

### Параллельные треки (ничей impl никого не блокирует)

#### Track P1 — Frontend engine

> Подробности реализации каждой задачи — в [§История реализации](#история-реализации).

| # | Задача | impl / Разблокирует | НЕ блокирует |
|---|---|---|---|
| 1A | ✅ **`[P1]` Quirks-mode application** | Половина legacy-сайтов | 2026-05-20 |
| 1A.1 | ✅ unitless length quirk | `layout/src/style.rs:2945` | — |
| 1A.2 | ✅ IE7 line-height quirk | `layout/src/style.rs:5013` | — |
| 1A.3 | ✅ quirks test coverage | `layout/src/lib.rs:8449` | — |
| 1A.4 | ✅ HTML pres. hints: `<font size/face>`, `img hspace/vspace/border`, `align` | `layout/src/style.rs:5482` | 2026-05-20 |
| 1B | ✅ **`[P1]` Типизированные `Length`/`Color`** | P2 п.3A; P3 CSSOM | — |
| 1B.1 | ✅ Length type через все декларации каскада | `layout/src/style.rs` | 2026-05-19 |
| 1B.2 | ✅ Color type через все декларации каскада | `layout/src/style.rs` | 2026-05-19 |
| 2A | ✅ **`[P1+P2]` Stacking contexts impl** | P2 п.2A | — |
| 2B | ✅ **`[P2+P1]` Property trees построение** | P2 п.1B | — |
| 3A | ✅ **`[P1+P2+P3]` Web Animations interpolation** | P2 п.3B; P3 scheduling | 2026-05-20 |
| ~~3B~~ | ✅ **`[P1+P3]` Push-tokenizer + incremental tree builder** | P3 п.4B | — |
| 4A | ✅ **`[P1+P2]` `<picture>`/`srcset`/`sizes` finishing** | P3 lazy-loading | — |
| 4B | ✅ **`[P1]` CSS Grid + полный Flexbox** | Адаптивные сайты | 2026-05-20 |
| 4B.1 | ✅ flex-direction + flex-wrap properties | `layout/src/style.rs` | — |
| 4B.2 | ✅ flex-grow + flex-shrink + flex-basis | `layout/src/style.rs` | — |
| 4B.3 | ✅ flex item layout pass (main/cross axis) | `layout/src/box_tree.rs` | — |
| 4B.4 | ✅ flex gap application | `layout/src/box_tree.rs:509` | — |
| 4B.5 | ✅ flex wrapping + multi-line | `layout/src/box_tree.rs:509` | — |
| 4B.6 | ✅ CSS Grid properties (GridTrackSize/GridLine/GridAutoFlow) | `layout/src/style.rs` | — |
| 4B.7 | ✅ CSS Grid layout (lay_out_grid + blockify) | `layout/src/box_tree.rs` | — |
| 4C | ✅ **`[P1]` CSS Positioned Layout** | position: relative/absolute/fixed + inset | 2026-05-20 |
| 4C.1 | ✅ top/right/bottom/left + inset shorthand | `layout/src/style.rs` | — |
| 4C.2 | ✅ position: relative offset (shift_tree) | `layout/src/box_tree.rs` | — |
| 4C.3 | ✅ position: absolute/fixed (out-of-flow + PCB) | `layout/src/box_tree.rs` | — |
| 5 | ✅ **`[P1]` ICU4x segmenter + linebreak** | CJK типографика | — |
| 5.1 | ✅ ICU4x struct + segmenter init | `encoding/src/unicode_provider.rs` | — |
| 5.2 | ✅ line_break_opportunities impl | `encoding/src/unicode_provider.rs` | — |
| 5.3 | ✅ grapheme_boundaries impl | `encoding/src/unicode_provider.rs` | — |
| 5.4 | ✅ word_boundaries impl | `encoding/src/unicode_provider.rs` | — |
| 5.5 | ✅ bidi_runs impl | `encoding/src/unicode_provider.rs` | — |

#### Phase 1 ✅ (complete)

| # | Задача | impl / Разблокирует | Дата |
|---|---|---|---|
| 8G.1 | ✅ **`[P1+P3]` lumen-a11y-full stage 1** | ARIA role mapping (67 variants in `a11y/src/roles.rs`), text alternatives (accname §4) | Phase 1 ✅ 2026-05-27 |
| 8G.2 | ✅ **`[P1+P3]` lumen-a11y-full stage 2** | ARIA attributes (aria-current/modal/roledescription/valuenow/controls/owns/flowto/details), NodeId storage | Phase 1 ✅ 2026-05-27 |
| 8G.3 | ✅ **`[P1+P3]` lumen-a11y-full stage 3** | Full computed role mapping HTML-AAM, focus model, form control text alternatives, 125 tests PASS | Phase 1 ✅ 2026-05-27 |
| 10B | ✅ **`[P1+P3]` Invariant 1: DOM arena serialization** | `NodeId(u32)` no `Rc<RefCell>`, `Document::to_bytes`/`from_bytes` via bincode for T3 snapshot | ADR-008 Phase 1 ✅ 2026-05-27 |
| 10D.1 | ✅ **`[P1]` Invariant 3a: layout pure audit** | No `static MUT` / `lazy_static!` / `OnceCell` in `lumen-layout/src` hot path | ADR-008 Phase 1 ✅ 2026-05-27 |
| 10D.2 | ✅ **`[P1]` Invariant 3b: paint pure audit** | `paint/src/display_list.rs` pure-function requirement met (only comment marker present) | ADR-008 Phase 1 ✅ 2026-05-27 |
| 9D.1 | ✅ **`[P1+P2]` Canvas randomization** | `CanvasNoiseGenerator` LCG RNG in `canvas/src/fp_noise.rs`, per-session seed, 20 tests | ADR-007 Phase 1 ✅ 2026-05-28 |
| 9D.2 | ✅ **`[P1+P2]` WebGL normalize** | `GpuFingerprint` struct in `paint/src/fingerprint.rs`, adapter normalization, 5 tests | ADR-007 Phase 1 ✅ 2026-05-28 |
| 10F | ✅ **`[P1+P2+P3]` GPU layer LRU** | `LayerCache` in `paint/src/layer_cache.rs`, LRU + GPU memory budget, 7 tests | ADR-008 Phase 1 ✅ 2026-05-28 |
| 10G | ✅ **`[P1+P2+P3]` Glyph atlas eviction** | LRU eviction in `paint/src/atlas.rs` (`get_lru_candidates`, `remove_keys`), 4 tests | ADR-008 Phase 1 ✅ 2026-05-27 |

#### Phase 2-3 (planned)

| # | Задача | impl / Разблокирует | Дата |
|---|---|---|---|
| 6+ | 🟡 **`[P1+P3]` Shadow DOM / Accessibility / Forms / GC extended** | Advanced contenteditable, form validation, accessibility full integration | Phase 2-3. P1: Shadow DOM ✅, Forms ✅, GC ✅, Selection API ✅ (2026-05-30). P3: native pickers, validation tooltip UI, platform a11y bridges |
| 6.1 | ✅ contenteditable drag-drop + paste + undo/redo | Input dispatch coordination with shell | Phase 2 ✅ P1 done (2026-05-28); Phase 3 P3 shell integration pending |
| 6.2 | ✅ accessibility forms validation + visualization | Constraint validation in accessibility tree | Phase 1-3 ✅ P1 done (2026-05-28); P3 pending |
| 6.3 | ✅ ime-input composition events + ranges | Keyboard input for CJK/Cyrillic | Phase 1-3 ✅ P1 done (2026-05-31); Phase 2-3 P3 shell integration pending |
| 6.4 | ✅ svg-layout advanced transforms + viewport nesting | SVG aspect-ratio preservation | Phase 1-3 ✅ P1 done (2026-05-30); Phase 4 ✅ P2 done (2026-05-29): DrawSvgPath + tessellator |
| 6.5 | ✅ print-pdf advanced @page margin boxes + headers/footers | Full print pipeline from margin-box content | Phase 1-4 ✅ P1 done (2026-05-31); P2 inline content rendering pending |
| 6.6 | ✅ animation keyframe easing (cubic-bezier/steps) | Full timing functions support | Phase 1-2 ✅ P1 done (2026-05-20); complete in Phase 0 |
| 6.7 | ✅ transition advanced (interrupted/fill-mode/grouped) | Animation lifecycle completeness | Phase 1-3 ✅ P1 done (2026-05-28) |
| 6.8 | ✅ **`[P1+P3]` font-loading API** | @font-face lifecycle, FontFace interface, document.fonts | Phase 1 ✅ P1 done; Phase 2 ✅ JS bindings (_lumen_fonts_*); Phase 3 P3 pending |
| 6.9 | 🟡 **`[P1+P3]` performance-observer timing** | PerformanceEntry/PerformanceObserver types, mark/measure, observer delivery | Phase 1 ✅ P1 done (DOM types + mark/measure); Phase 2+ P3 JS binding + observer callback |

#### Track P2 — Backend rendering

| # | Задача | impl / Разблокирует | НЕ блокирует |
|---|---|---|---|
| ~~1A~~ | ✅ **`[P2]` Font fallback/matcher** | — | — |
| 1B | ✅ **`[P2+P1]` Compositor thread + layer tree** | Off-main-thread scroll | shell-интеграция active_tree() — P3 |
| 1B.1 | ✅ CompositorThread struct + spawn loop | `paint/src/compositor.rs:277` | — |
| 1B.2 | ✅ vsync tick-loop 60fps | `paint/src/compositor.rs:277` | — |
| 1B.3 | ✅ PushBlendMode/PopBlendMode pipeline Phase 0 | `paint/src/renderer.rs:1834` | — |
| 1B.4 | ✅ off-screen opacity layer rendering | `paint/src/renderer.rs` | 2026-05-19 |
| 1B.5 | ✅ GPU texture upload for layer snapshots | `paint/src/renderer.rs` | 2026-05-19 |
| 2A | ✅ **`[P1+P2]` Painting order traversal** | — | — |
| 2B | ✅ **`[P2]` Stacking-aware hit testing** | P3 shell input handler | — |
| 3A | ✅ **`[P2]` Color management + Display P3/Rec2020** | Фотографии с P3-профилем | Только `lumen-paint` |
| 3A.1 | ✅ ColorSpace enum in ComputedStyle | `layout/src/style.rs` | 2026-05-20 |
| 3A.2 | ✅ Display P3 parsing in CSS color functions | `layout/src/style.rs` | 2026-05-20 |
| 3A.3 | ✅ HDR tone-mapping utilities (sRGB↔P3) | `layout/src/style.rs` | 2026-05-20 |
| 3A.4 | ✅ ColorFloat variant (f32 channels) | `layout/src/style.rs` | 2026-05-20 |
| 3A.5 | ✅ color space awareness in renderer | `paint/src/display_list.rs` | 2026-05-20 |
| 3B | ✅ **`[P1+P2+P3]` Web Animations compositor offload** | Smooth-анимации | 2026-05-20 |
| 3B.1 | ✅ CompositorAnimFrame + CompositorOverride types | `layout/src/animation.rs` | 2026-05-20 |
| 3B.2 | ✅ AnimationFrame::to_compositor_frame() | `layout/src/animation.rs` | 2026-05-20 |
| 3B.3 | ✅ transform_fns_to_matrix helper | `layout/src/property_trees.rs` | 2026-05-20 |
| 3B.4 | ✅ build_display_list_with_anim + walk_with_anim | `paint/src/display_list.rs` | 2026-05-20 |
| 3B.5 | ✅ shell wires anim_frame → DL rebuild without relayout | `shell/src/main.rs` | 2026-05-20 |
| 4 | ✅ **`[P1+P2]` mix-blend-mode/backdrop-filter pipeline** | Современные UI-эффекты | 2026-05-20 |
| 4.5 | ✅ **`[P2]` Transform pipeline (DisplayCommand + renderer)** | P4 transform matrix | 2026-05-20 |
| 4.5.1 | ✅ PushTransform/PopTransform DisplayCommand | `paint/src/display_list.rs` | 2026-05-20 |
| 4.5.2 | ✅ forward_box_transform публичный из layout | `layout/src/property_trees.rs` | 2026-05-20 |
| 4.5.3 | ✅ transform_stack + CPU-side vertex transformation | `paint/src/renderer.rs` | 2026-05-20 |
| 5A | ✅ **Canvas 2D basic context** — CPU rasterizer, `CanvasRenderingContext2D` Phase 0 | `engine/canvas/` | 2026-05-22 |
| 5B | ✅ **WOFF2/WOFF1 decoder** — brotli + zlib, glyf transform, sfnt rebuild | `engine/font/src/woff2.rs` | 2026-05-22 |
| 5+ | ✅ **GPU linear/radial gradient pipeline** — WGSL шейдер + CPU uniform + DrawOp::Gradient | `paint/src/renderer.rs` | 2026-05-22 |
| 5++ | ✅ **Extras**: object-fit ✅, variable fonts ✅, Print PDF Phase 1 (✅ pagination module) | `layout/src/pagination.rs` | 2026-05-28 |

#### Track P3 — Runtime + system (объединённый домен — больше треков, но всё параллельно)

| # | Задача | impl / Разблокирует | НЕ блокирует |
|---|---|---|---|
| 1B | ✅ **`[P3]` rquickjs integration scaffold** | Forms, Animations, SWs, DevTools | `crates/js/` |
| 2A | 🟡 **`[P3]` SOP/CORS/mixed-content/sandbox** | Публичная сеть | Только network + shell |
| 2A.1 | ✅ block blockable in HttpClient::fetch | `network/src/lib.rs:1478` | — |
| 2A.2 | ✅ sandbox_flags on iframe DOM element | `dom/src/lib.rs` | — |
| 2A.3 | ✅ script execution gate in shell | `shell/src/main.rs:752` | — |
| 2A.4 | ✅ form submission gate | `dom/src/lib.rs` | — |
| 2A.5 | ✅ navigation restriction enforcement | `dom/src/lib.rs` + `shell/src/main.rs` | — |
| 2C | ✅ **`[P3]` Tab session export/import** | UX | `storage/src/session_export.rs` | JSON v1, --import-session, auto-save на close, 12 тестов |
| 3A | ✅ **`[P3]` DPR + scroll в shell** | 4K + длинные страницы | shell + paint |
| 3A.1 | ✅ relayout-on-resize | `shell/src/main.rs` | — |
| 3A.2 | ✅ horizontal scroll | `shell/src/main.rs` | — |
| 3A.3 | ✅ momentum scroll | `shell/src/main.rs` | — |
| 3B | ✅ **`[P3]` HTML event loop в Lumen-loop** | P1/P2 rAF | Только `lumen-shell::runtime` |
| 3B.1 | ✅ reload via queue_task | `shell/src/main.rs` | — |
| 3B.2 | ✅ rendering steps ordering | `shell/src/main.rs` | — |
| 3B.3 | ✅ real observers | `shell/src/main.rs` | — |
| 4A | ✅ **`[P3]` JS↔DOM bindings** (после 1B) | Любая JS-динамика | Phase 0: getElementById/querySelector/textContent/setAttribute/createElement/appendChild |
| 4B | ✅ **`[P1+P3]` Streaming pipeline shell-side** | Первый кадр без задержки | EventLoop\<LoadEvent\>: окно сразу, HTML bytes-chunks → feed_bytes → IncrementalTreeBuilder, промежуточные кадры каждые 150 мс; encoding decode один раз в финальном pipeline |
| 4B.1 | ✅ preload scan call before DOM parse | `shell/src/main.rs:688` | — |
| 4B.2 | ✅ preload hint dispatcher | `shell/src/main.rs` | — |
| 4B.3 | ✅ preload URL resolution | `shell/src/main.rs` | — |
| 4B.4 | ✅ preload fetch deduplication | `shell/src/main.rs` | — |
| 4B.5 | ✅ preload priority + EventSink | `shell/src/main.rs` | — |
| 4B.6 | ✅ feed_bytes in IncrementalTreeBuilder + raw byte chunks in shell | `html-parser/src/tree_builder.rs`, `shell/src/main.rs` | — |
| 5A | ✅ **`[P3]` HTTP/2** | Latency | Только network |
| 5A.1 | ✅ ALPN h2 negotiation | `network/src/lib.rs` | h2 → Err placeholder, http/1.1 fallback |
| 5A.2 | ✅ Frame codec (RFC 9113 §6) | `network/src/h2/frame.rs` | 10 типов + Unknown, padding strip, +46 тестов |
| 5A.3 | ✅ HPACK (RFC 7541) | `network/src/h2/hpack.rs` | static+dynamic table, Huffman, integer enc, +24 тестов (RFC C.3/C.4 vectors) |
| 5A.4 | ✅ Connection + concurrent streams | `network/src/h2/conn.rs` | preface, SETTINGS, fetch (single-stream), send_request + read_response_for_stream (concurrent), +16 тестов |
| 5A.5 | ✅ Pool multiplexing | `network/src/h2/pool.rs` | H2Pool acquire/release/evict, интеграция в fetch_single |
| 5A.6 | ✅ Flow control + WINDOW_UPDATE | `network/src/h2/conn.rs` | WINDOW_UPDATE после каждого DATA, +3 теста |
| 5A.7 | ✅ HTTP response cache (RFC 7234) | `network/src/http_cache.rs` | Cache-Control + ETag + If-None-Match + heuristic freshness, +19 тестов |
| 5B | ✅ **`[P3]` HTTP Range requests** | `<video>` seek | — |
| 5C | ✅ **`[P3]` DevTools/CDP минимум** | Debug движка | `crates/devtools/` | WS сервер + Browser.getVersion + DOM.getDocument stub + --devtools-port |
| 6+ | ⬜ **`[P3]` knowledge / Profiles / Focus / IME / WebSockets / SW / V8 / AI** (Phase 2-3) | — | — |
| 7A | ⬜ **`[P3]` Tab UX** (§12.13, Phase 2) | Современная модель вкладок | `shell/src/tabs/` |
| 7A.1 | ⬜ Vertical tabs panel (toggle, drag-reorder, collapse) | `shell/src/tabs/vertical.rs` | — |
| 7A.2 | ⬜ Tree-style tabs (parent-child) | `shell/src/tabs/tree.rs` | — |
| 7A.3 | ⬜ Workspaces (изолированные группы) | `shell` + `storage/src/workspaces.rs` | — |
| 7A.4 | ⬜ **`[P3+P2]` Split view** (2-4 viewport на окно) | `shell` + `paint` multi-viewport | требует координации с P2 |
| 7A.5 | ⬜ Tab auto-archive (UX-фича: убрать вкладки старше 12 ч из tab strip в @archive) | `shell/src/tabs/archive.rs` | **семантика отделена от трека 10**: 7A.5 — UI-скрытие, **трек 10** — RAM-выгрузка по tier'ам |
| 7B | ⬜ **`[P3]` Power-user input** (§12.14, Phase 2-3) | Keyboard-first аудитория | `shell/src/input/` |
| 7B.1 | ⬜ Vim-style key bindings (modal) | `shell/src/input/vim.rs` | — |
| 7B.2 | ⬜ **`[P3+P1]` Click-hint overlay** | `shell` + layout-итератор clickable | требует P1: iterator по clickable в `lumen-layout` |
| 7B.3 | ⬜ Mouse gestures | `shell/src/input/gestures.rs` | — |
| 7B.4 | ⬜ Custom omnibox aliases | `shell` + user config | — |
| 7B.5 | 🟡 **`[P3+P1]` Find-in-page с regex** | `shell` + visible-text итератор | **P1 done** — `collect_visible_text` + `TextFragment` в `lumen-layout::text_iter`; P3 — regex UI + highlight overlay |
| 7C | 🟡 **`[P3]` Privacy UX** (§12.15, Phase 2) | Встроенная защита | `lumen-network::filter` + `shell` |
| 7C.1 | ✅ Block list engine (EasyList + hosts files) | `network/src/filter/easylist.rs` + `hosts.rs` + `CompositeFilter` | P1 done 2026-05-31: 26 тестов |
| 7C.2 | ⬜ Per-site permission UI panel | `shell/src/site_settings/` | — |
| 7C.3 | ⬜ Cookie-banner auto-dismiss | `shell/src/cookies/banner.rs` | использует `JsRuntime` |
| 7C.4 | ⬜ Shields toolbar widget (счётчик блокировок) | `shell/src/toolbar/shields.rs` | — |
| 7D | ⬜ **`[P3]` Web platform baseline** (§12.16, Phase 2-3) | Современная авторизация и изоляция | `lumen-network` + `shell` |
| 7D.1 | ⬜ Passkeys / WebAuthn (CTAP2 client + navigator.credentials) | `network/src/webauthn.rs` + `js/src/credentials.rs` | новый trait `CredentialProvider` |
| 7D.2 | ⬜ Tab containers (storage partitioning) | `storage/src/partition.rs` + `shell` | — |
| 7D.3 | ⬜ Sidebar web panels (мини-страница в sidebar) | `shell/src/sidebar/web_panel.rs` | — |
| 7E | ⬜ **`[P3]` DevTools полный** (§12.12, Phase 4+) | Поверх существующего CDP-минимума (5C) | `crates/devtools/` |
| 7E.1 | ⬜ DOM inspector panel (tree + attributes) | `devtools` + read из `lumen-dom` | — |
| 7E.2 | ⬜ **`[P3+P4]` Computed styles panel** | сериализация `ComputedStyle` | P4: expose ComputedStyle как serializable JSON |
| 7E.3 | ✅ **`[P3+P2]` Box model overlay** (margin/border/padding overlay) | через display list | P2: overlay primitive в `DisplayCommand` — 2026-05-29 |
| 7E.4 | ⬜ Network panel (live request log) | `devtools` слушает `NetworkTransport` events | — |
| 7E.5 | ⬜ JS console (eval в контексте страницы) | `devtools` + `JsRuntime::eval` | — |
| 8 | ⬜ **`[P3]` Automation API** (§6.11, [ADR-006](docs/decisions/ADR-006-automation-api.md)) | First-class automation surface, фундамент собственных тестов и внешних клиентов | `lumen-driver`, `lumen-mcp-server`, `lumen-bidi-server` |
| 8A | ✅ **`[P3]` `lumen-driver` крейт + `BrowserSession` trait + `InProcessSession`** (Phase 0) | 8A.1–8A.6 ✅; 8A.7 ⬜ (Phase 4) | `crates/driver/` |
| 8A.1 | ✅ `BrowserSession` trait в `lumen-core::ext` + `NullBrowserSession` заглушка (object-safe, `Send`) | `core/src/ext.rs:1514` | 2026-05-29 |
| 8A.2 | ✅ `InProcessSession` impl | `driver/src/session.rs:53` | 2026-05-28 |
| 8A.3 | ✅ **`[P3+P2]` Off-screen рендер** (`Renderer::render_to_image() -> Image`) | `paint/src/renderer.rs` | P2/P3: `new_headless()` без winit + `render_to_image()` GPU readback — 2026-05-27 |
| 8A.4 | ✅ **`[P3+P1]` Structural getters**: `layout_box`, `computed_style` через handle / selector | `layout` exposers: `find_box_by_selector`, `computed_style_by_selector`, `ComputedStyleSnapshot` | 2026-05-27 |
| 8A.5 | ✅ Software rasterizer для тестов (`tiny-skia` opt-in под `cfg(test)`) | `paint/src/cpu_raster.rs` | детерминизм пикселей Windows/macOS/Linux CI — 2026-05-27 |
| 8A.6 | ✅ Миграция `graphic_tests/`: structural-assert Rust-тесты + `graphic_tests/snapshots/*.png` эталоны | `driver/tests/test_00..49.rs` + `snapshot_cpu.rs` (57 страниц) | все 57 html-страниц graphic_tests покрыты CPU-снапшотами; `cargo test -p lumen-driver --features cpu-render` — 2026-05-31 |
| 8A.7 | ⬜ Шелл переписать как первого клиента trait-а (winit/wgpu → один из транспортов) | `shell/src/main.rs` | — |
| 8B | ✅ **`[P1]` `lumen-mcp` крейт** (Phase 1) | AI-агенты (Claude/GPT/Browser Use) без обёрток | `crates/mcp/` |
| 8B.1 | ✅ JSON-RPC over stdio + TCP socket transport | `mcp/src/transport.rs` | P1 done 2026-05-31: StdioTransport + TcpTransport + VecTransport (tests) |
| 8B.2 | ✅ Resources: `screenshot`, `a11y_tree`, `layout`, `console`, `network` | `mcp/src/server.rs` | P1 done 2026-05-31 |
| 8B.3 | ✅ Tools: `click`, `type`, `scroll`, `navigate`, `wait`, `eval`, `query` | `mcp/src/server.rs` | P1 done 2026-05-31 |
| 8B.4 | ✅ `lumen --mcp` / `--mcp-port N` CLI flags | `shell/src/main.rs` | P1 done 2026-05-31: extract_mcp_mode + run_mcp_mode |
| 8C | ⬜ **`[P3+shell]` Native input injection** (Phase 1) | Не-distinguishable от пользователя input, для AI-агентов и тестов | `shell/src/input/native.rs` |
| 8C.1 | ⬜ Mouse/keyboard events идут в event loop тем же путём, что winit-события от ОС | `shell/src/main.rs` | НЕ через JS `dispatchEvent` |
| 8C.2 | ✅ `event.isTrusted = true` для native-injected events | `dom/src/lib.rs` | 2026-05-28 |
| 8D | ✅ **`[P1]` Auto-wait inside engine** (Phase 1) | Anti-flake, замена SDK retry-loops | `driver/src/session.rs` | P1 done 2026-05-31 |
| 8D.1 | ✅ `wait_for(Cond::Visible)` — border_box.width/height > 0; display:none → no layout box → false | layout-аware | P1 done 2026-05-31 |
| 8D.2 | ✅ `wait_for(Cond::NetworkIdle)` — active_network_requests counter; saturating_sub after HTTP fetch | network-аware | P1 done 2026-05-31 |
| 8D.3 | ✅ `wait_for(Cond::JsIdle)` — pending_js_microtasks counter; set_pending_js_tasks() for shell hook | shell runtime-аware | P1 done 2026-05-31 |
| 8E | ⬜ **`[P3]` Per-context isolation by default** (Phase 1) | Cookies/storage/cache/viewport/UA/fingerprint per session | `driver/src/context.rs` |
| 8F | ⬜ **`[P3]` Deterministic mode** (Phase 1) | Repeatable tests, опирается на §9.5 anti-fp инфраструктуру | `driver/src/determinism.rs` |
| 8F.1 | ⬜ `set_clock(ClockMode::Frozen / Real / Monotonic)` | shell timers + Performance.now bridge | — |
| 8F.2 | ⬜ `set_rng_seed(u64)` — детерминированный `Math.random()` | JS runtime hook | — |
| 8F.3 | ⬜ `freeze_fingerprint(profile)` — фиксированные canvas/WebGL/audio/font enumeration | §9.5 anti-fp + bundled-only fonts mode | — |
| 8G | ✅ **`[P3+P1]` A11y tree first-class** (Phase 1, **зависит от P1 `lumen-a11y`**) | Semantic locator surface для tests + AI agents | `lumen-a11y` published interface. P1 done 2026-05-31: `AXRole::as_str()`, `A11yState`, enriched `A11yNode` (node_id/description/placeholder/state), `a11y_tree()` uses `build_ax_tree()`, 14 тестов |
| 8G.1 | ✅ A11y tree доступна через `BrowserSession::a11y_tree()` | `driver/src/session.rs` uses `lumen_a11y::build_ax_tree()` | P1 done 2026-05-31 |
| 8G.2 | ✅ `Query::Role { role, name }` matching по a11y-tree (Playwright-стиль `getByRole`) | `driver/src/session.rs` `find_a11y_node`/`find_all_a11y_nodes` + `matches_query` | P1 done 2026-05-31 |
| 8H | ⬜ **`[P3]` `lumen-bidi-server` крейт** (Phase 2) | Playwright/Selenium 5/Cypress «из коробки» | `crates/bidi/` |
| 8H.1 | ⬜ WebSocket transport + W3C BiDi handshake | `bidi/src/transport.rs` | — |
| 8H.2 | ⬜ BiDi modules core: `session`, `browsingContext`, `script`, `network`, `input` | `bidi/src/modules/` | W3C Working Draft, May 2026 |
| 8H.3 | ⬜ **Ship BiDi gaps** (см. ADR-006): response body, locale/timezone/offline, per-context UA, viewport-before-popup, preload per-context, download lifecycle, cookie change events, per-origin clear | `bidi/src/extensions.rs` | gap-mapping в `subsystems/lumen-bidi-server.md` |
| 8H.4 | ⬜ `lumen --bidi-port N` CLI flag | `shell/src/cli.rs` | — |
| 8I | ⬜ **`[P3]` `lumen-cdp-shim` крейт** (Phase 3+, **opt-in, по реальному запросу**) | Legacy Puppeteer-совместимость | `crates/cdp-shim/` |
| 9 | 🟡 **`[P1]` Anti-detection privacy stack** (§9.5, [ADR-007](docs/decisions/ADR-007-anti-detection-stack.md)) | Privacy by default; устойчивость к Cloudflare/DataDome/Akamai false-positive. 9A ✅ Layer 1 (P1 2026-05-31); 9B-9C ⬜ TLS/HTTP fingerprint | `lumen-network`, `lumen-js`, `lumen-shell`, `lumen-paint` (минимально), `lumen-canvas` |
| 9A | ✅ **`[P1]` Layer 1: surface API без automation-маркеров** (Phase 1) | navigator.webdriver отсутствует; нет chrome.runtime/cdc_*/__playwright/etc.; event.isTrusted=true для native input; nav.appName/vendor/product/plugins/mimeTypes совместимы с Chrome | `lumen-js/src/surface_api.rs` P1 done 2026-05-31 |
| 9A.1 | ✅ Audit JS bindings + `install_surface_api_protection` (hardening shim) | `js/src/surface_api.rs` (11 unit) + `js/tests/no_automation_markers.rs` (19 runtime) | — |
| 9A.2 | ✅ Negative tests: `webdriver` absent, no automation globals, isTrusted, standard browser props | `js/tests/no_automation_markers.rs` (19 тестов); source audit — `driver/tests/antidetect_surface_api.rs` (7 тестов) | — |
| 9B | ⬜ **`[P3]` Layer 2: TLS fingerprint Chrome-matching** (Phase 1) | JA3/JA4 как у current stable Chrome; per-profile override | `lumen-network` rustls config |
| 9B.1 | ⬜ Cipher suite ordering matching Chrome | `network/src/tls/fingerprint.rs` | — |
| 9B.2 | ⬜ Extension list + supported groups matching Chrome | `network/src/tls/fingerprint.rs` | — |
| 9B.3 | ⬜ ALPN order `h2`, `http/1.1` matching Chrome | `network/src/tls/fingerprint.rs` | — |
| 9B.4 | ⬜ JA3/JA4 snapshot test против известных Chrome JA3 | `network/tests/ja3_match.rs` | обновляется per Chrome major release |
| 9B.5 | ⬜ Per-profile TLS config (Standard / Strict / Tor) | `network/src/tls/profiles.rs` | — |
| 9C | ⬜ **`[P3]` Layer 3: HTTP fingerprint Chrome-matching** (Phase 1) | Header order + casing + HTTP/2 SETTINGS как у Chrome | `lumen-network` http/h2 |
| 9C.1 | ⬜ HTTP/1.1 header order + casing matching Chrome | `network/src/http/headers.rs` | — |
| 9C.2 | ⬜ HTTP/2 SETTINGS frame values matching Chrome | `network/src/h2/settings.rs` | — |
| 9C.3 | ⬜ HTTP/2 stream priority pattern matching Chrome | `network/src/h2/priority.rs` | — |
| 9C.4 | ⬜ Accept-Language default `en-US,en;q=0.9` (не палит реальную локаль) | `network/src/http/headers.rs` | — |
| 9C.5 | ⬜ Client Hints handling (опционально, выключено на Strict) | `network/src/http/client_hints.rs` | — |
| 9D | ⬜ **`[P3+P2]` Layer 4: rendering fingerprint** (Phase 2) | Canvas/WebGL/audio randomization, Battery API disable, WebRTC mDNS-only | `lumen-canvas`, `lumen-paint`, `lumen-js` |
| 9D.1 | ✅ Canvas randomization (Brave-style per-session seed) | `canvas/src/fp_noise.rs` | — |
| 9D.2 | 🟡 WebGL renderer/vendor normalization | `js/src/webgl_bindings.rs` | P1 done: GpuFingerprint normalization (paint/fingerprint.rs), JS stub (_LUMEN_GPU_VENDOR/_RENDERER); P3 pending: wire to getParameter(UNMASKED_VENDOR/RENDERER_WEBGL) |
| 9D.3 | ✅ AudioContext fingerprint noise | `js/src/audio_bindings.rs` | 2026-05-30 |
| 9D.4 | ✅ Battery API disable on Strict | `js/src/battery_bindings.rs` | 2026-05-30: navigator.getBattery() → rejected Promise, 4 unit-тестов |
| 9D.5 | ⬜ WebRTC mDNS-only host candidates | `network/src/webrtc/candidates.rs` | при наличии WebRTC; иначе noop |
| 9D.6 | ✅ Hardware concurrency / screen / timezone normalization per profile | `js/src/navigator_bindings.rs` | 2026-05-30: hardwareConcurrency=2, deviceMemory=8, platform=Win32, screen 1920×1080, getTimezoneOffset→0, 10 unit-тестов |
| 9E | ⬜ **`[P3]` Layer 5: behavioral mimicry (opt-in)** (Phase 1, **для automation API**) | `InputMode::HumanLike` для тестировщиков | `shell/src/input/humanlike.rs` |
| 9E.1 | ⬜ Bézier-curve mouse paths between coordinates | `shell/src/input/humanlike.rs` | — |
| 9E.2 | ⬜ Variable inter-keystroke timing (Gaussian) | `shell/src/input/humanlike.rs` | — |
| 9E.3 | ⬜ Pre-click dwell time | `shell/src/input/humanlike.rs` | — |
| 9F | ⬜ **`[P3]` Layer 6: профили Standard/Strict/Tor** (Phase 2) | Per-profile config + per-context override через BrowserSession | `lumen-storage/src/profiles/fingerprint.rs` |
| 9F.1 | ⬜ Профильный конфиг fingerprint (объединяет слои 2/3/4) | `storage/src/profiles/fingerprint.rs` | — |
| 9F.2 | ⬜ `BrowserSession::set_fingerprint_profile(profile)` per-context override | `driver` + `core::ext` | связка с ADR-006 task 8F.3 |
| 9F.3 | ⬜ Tor-mode профиль (Tor circuit + Tor Browser JA3/UA/screen/fonts pinning + no persistent state) | `storage` + `network` + `shell` | Phase 3, отдельная задача |
| 9G | ⬜ **Red lines + perf gate — code-level enforcement** | Чтобы trigger-PR случайно не нарушил ADR-006 / ADR-007 | — |
| 9G.1 | ⬜ CI lint: запрет имён `*captcha*`, `*solver*`, `*ip_rotation*`, `*proxy_pool*` в crate-names | `.github/workflows/lint.yml` | — |
| 9G.2 | ⬜ README / маркетинговые тексты не используют «scraping», «stealth», «bypass» — линтер в CI | `.github/workflows/marketing-words.yml` | — |
| 9G.3 | ⬜ **CI bench gate**: `cargo run -p lumen-bench --release` + сравнение с `bench/baseline.json` (median, p95) → fail PR при регрессе >5% в default-сборке. Применяется к PR, затрагивающим `lumen-driver` / `lumen-mcp-server` / `lumen-bidi-server` / `lumen-network` / `lumen-canvas` / `lumen-js` / `lumen-storage::profiles` / `lumen-shell::input` | `.github/workflows/bench-gate.yml` + `bench/baseline.json` + `bench/compare.py` | binding по ADR-006 §«Performance gate» и ADR-007 §«Performance gate» |
| 9G.4 | ✅ **`bench/baseline.json` обновление-процедура**: разработчик руками `cargo run -p lumen-bench --release` затем обновляет JSON; коммит документирует архитектурное обоснование | `bench/UPDATE.md` | — |
| 9G.5 | ✅ **`lumen-bench` RAM-axis расширение** (требование [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)): добавить замеры `peak_rss`, `steady_state_rss` к существующим time-метрикам; get_rss_bytes() cross-platform (libc::getrusage на Unix, GetProcessMemoryInfo на Windows); baseline.json + UPDATE.md документация | `bench/src/main.rs` + `baseline.json` + `UPDATE.md` | binding по ADR-008 §«Performance gate» |
| 10 | ⬜ **`[P3]` Tab lifecycle: пятитайерная модель** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)) | Главный продуктовый дифференциатор по RAM: 50 вкладок ~400 MB vs Chrome 6-10 GB | `lumen-shell::tab_lifecycle`, `lumen-storage::tab_snapshot`, `lumen-core::ext::MemoryPressureSource` |
| 10A | ⬜ **`[P3]` `TabState` enum + state machine T0-T4** (Phase 1) | Базовая модель + transition triggers | `shell/src/tab_lifecycle/state.rs` |
| 10A.1 | ⬜ `enum TabState { Active, BackgroundRecent, BackgroundOld, Hibernated }` + transitions | `shell/src/tab_lifecycle/state.rs` | — |
| 10A.2 | ⬜ OR-of-conditions trigger: idle timeout + memory pressure + LRU within budget | `shell/src/tab_lifecycle/trigger.rs` | — |
| 10A.3 | ⬜ Per-user конфигурация таймаутов (5 мин / 30 мин / pinned-never) | `storage/src/profiles/tabs.rs` | — |
| 10B | ✅ **`[P3+P1]` Invariant 1: DOM arena serialization** | `NodeId(u32)` без `Rc<RefCell>`, `Document::to_bytes`/`from_bytes` via bincode | ADR-008 ✅ 2026-05-27 |
| 10B.1 | ✅ Audit `lumen-dom` — no `Rc<RefCell<Node>>`, всё на `NodeId(u32)` | `dom/src/lib.rs` | ADR-008 Invariant 1 ✅ |
| 10B.2 | ✅ `Document::to_bytes` / `Document::from_bytes` via bincode | `dom/src/lib.rs` | — |
| 10B.3 | ✅ Anti-pattern guard в `lumen-dom` | `dom/src/lib.rs` | — |
| 10C | ✅ **`[P3]` Invariant 2: JsRuntime suspend/resume API** | `pause/unpause/suspend/resume` в `JsRuntime` trait + QuickJS impl | ADR-008 ✅ 2026-05-27 |
| 10C.1 | ✅ `JsRuntime` trait: `pause()` / `unpause()` / `suspend()` / `resume()` | `core/src/ext/js.rs` | — |
| 10C.2 | ✅ Имплементация для `rquickjs` через `JS_WriteObject` / `JS_ReadObject` | `js/src/quickjs/suspend.rs` | — |
| 10C.3 | ⬜ zstd-сжатие heap snapshot; cap 5 MB/tab disk | `js/src/quickjs/snapshot.rs` | — |
| 10C.4 | ⬜ V8 compatibility note при миграции в Phase 3 | `docs/decisions/` | check before Phase 3 |
| 10D | ✅ **`[P3+P1+P2]` Invariant 3: pure layout + paint** | 10D.1+10D.2+10D.3 ✅ | `lumen-layout` + `lumen-paint` |
| 10D.1 | ✅ Audit `lumen-layout` — no `static MUT` / `lazy_static` / `OnceCell` в hot path | `layout/src/lib.rs` | ADR-008 Invariant 3 ✅ 2026-05-27 |
| 10D.2 | ✅ Audit `lumen-paint::display_list` — pure-function requirement met | `paint/src/display_list.rs` | ADR-008 Invariant 3 ✅ 2026-05-27 |
| 10D.3 | ✅ Cross-tab caches — `EvictableCache` trait + `CacheRegistry` в `lumen-core::ext`; `GlyphAtlas`/`ImageDecodeCache`/`LayerCache` impl; 8 тестов | `core/src/ext.rs` + `paint/src/atlas.rs` + `paint/src/layer_cache.rs` + `image/src/decode_cache.rs` | 2026-05-30 |
| 10E | 🟡 **`[P3]` T0 экономия: image decode cache LRU + viewport-gating** (Phase 1, **главный источник экономии**) | 10E.1+10E.2 ✅; 10E.4 ⬜ | `image/src/decode_cache.rs` |
| 10E.1 | ✅ `ImageHandle` (`Arc<Image>`) + `ImageKey` — тонкий ref, экспортирован из `lumen-image` | `image/src/decode_cache.rs` | 2026-05-30 |
| 10E.2 | ✅ `ImageDecodeCache` с LRU + memory budget (256 MB default), 9 unit-тестов | `image/src/decode_cache.rs` | 2026-05-30 |
| 10E.3 | ✅ Viewport-gating в layout: `gate_image_requests(root, viewport, scroll_x, scroll_y)` — AABB intersection, HashSet<NodeId>, 7 тестов | `layout/src/image_gating.rs` | 2026-05-29 |
| 10E.4 | ⬜ Scroll-discard: при удалении от viewport на >3 экрана — handle освобождается | `shell/src/scroll/decode_gating.rs` | — |
| 10F | ✅ **`[P1+P3]` T0 экономия: GPU layer LRU + texture recycling** | `LayerCache` + texture pool — 7+2 тестов | `paint/src/layer_cache.rs` |
| 10F.1 | ✅ `LayerCache` с LRU + GPU memory budget | `paint/src/layer_cache.rs` | — |
| 10F.2 | ✅ Texture pool recycling (одна `wgpu::Texture` переиспользуется для разных layers) | `paint/src/texture_pool.rs` | P3 lifecycle integration pending |
| 10G | ✅ **`[P1+P2+P3]` T0 экономия: glyph atlas LRU eviction** | LRU eviction в `paint/src/atlas.rs` (`get_lru_candidates`, `remove_keys`) — 4 теста | `paint/src/atlas.rs` |
| 10H | ✅ **`[P2]` `MemoryPressureSource` trait** (Phase 1) | OS-сигналы памяти управляют tier-переходами | `core/src/ext.rs` + `core/src/memory_pressure.rs` |
| 10H.1 | ✅ Trait + `enum MemoryPressureLevel { Low, Medium, High }` + `NullMemoryPressureSource` | `core/src/ext.rs` (ADR-008); 3 unit-тесты | — |
| 10H.2 | ✅ Win32 impl: `Win32MemoryPressureSource` — `GlobalMemoryStatusEx` polling | `core/src/memory_pressure.rs`; 1 unit-тест (live Windows call) | — |
| 10H.3 | ✅ Linux impl: `LinuxMemoryPressureSource` — `/proc/pressure/memory` PSI polling | `core/src/memory_pressure.rs`; 3 unit-тесты | — |
| 10H.4 | ✅ macOS impl: `MacosMemoryPressureSource` — `host_statistics64(HOST_VM_INFO64)` polling | `core/src/memory_pressure.rs`; 4 unit-тесты (threshold logic) | — |
| 10H.5 | ✅ Подписка кэшей на pressure events: `on_memory_pressure(level)` | `ImageDecodeCache` (4 тесты) + `GlyphAtlas` (3 тесты) + `LayerCache` (3 тесты) | — |
| 10I | ⬜ **`[P3]` T2 → SQLite JS heap snapshot persistence** (Phase 2) | JS heap на диск при T2; restore при T0 | `storage/src/tab_snapshot.rs` |
| 10I.1 | ⬜ Schema: `tab_snapshots(tab_id, js_heap_blob, dom_blob, scroll, form_state, ts)` | `storage/migrations/0NN_tab_snapshots.sql` | — |
| 10I.2 | ⬜ Async-save при T1→T2 (без блокирования UI) | `shell/src/tab_lifecycle/persist.rs` | — |
| 10I.3 | ⬜ Async-load при T2→T0 (с indeterminate UI hint если > 100 ms) | `shell/src/tab_lifecycle/restore.rs` | — |
| 10J | ⬜ **`[P3]` T3 hibernation: full DOM serialization** (Phase 2) | DOM в SQLite, в RAM только TabMetadata | `storage/src/tab_snapshot.rs` |
| 10J.1 | ⬜ DOM arena → bincode → zstd → SQLite blob | `dom/src/serialize.rs` + `storage/src/tab_snapshot.rs` | uses 10B.2 |
| 10J.2 | ⬜ `TabMetadata { url, title, scroll, favicon }` остаётся в RAM | `shell/src/tab_metadata.rs` | <200 KB target |
| 10J.3 | ⬜ Restore: deserialize → re-run scripts → full layout+paint (target ≤ 1500 ms) | `shell/src/tab_lifecycle/restore.rs` | — |
| 10K | ⬜ **`[P3]` UI affordance: индикация tier'а в tab strip** (Phase 2) | Пользователь видит, что вкладка спит | `shell/src/tabs/strip_ui.rs` |
| 10K.1 | ⬜ Иконка "Z" / fade-opacity на T2/T3 tabs | `shell/src/tabs/strip_ui.rs` | — |
| 10K.2 | ⬜ Tooltip "Вкладка спит — клик восстановит за ~1 сек" с показом tier'а | `shell/src/tabs/tooltip.rs` | — |
| 10K.3 | ⬜ Loading-spinner при restore > 200 ms | `shell/src/tabs/restore_ui.rs` | — |
| 10L | ⬜ **`[P3]` JS heap GC tuning per tier** (Phase 2) | Активная — мягкий GC, idle — агрессивный | `js/src/gc_policy.rs` |
| 10M | ⬜ **`[P3]` `samples/heavy.html`** — Habr-style тестовая страница для бенчей T0-heavy | `samples/heavy.html` | используется в `lumen-bench` |

---

### Точки координации (минимум, явно)

| Когда | Кто с кем | Что координируется |
|---|---|---|
| После Sprint 0 | Все трое | Зафиксированы контракты — больше совместного PR нет до точек ниже. |
| После P1 п.2A | P1 → P2 | P1 публикует «stacking impl готов» в commit-body; P2 валидирует, что painting order на реальных данных корректен (visual sample). Один день. |
| После P1 п.2B | P1 → P2 | Property trees: P2 включает их в compositor commit. Один день. |
| После P1 п.3A + P2 п.3B | P1 ↔ P2 | Web Animations: stub-interpolator меняем на реальный. Один день. |
| После P1 п.3B + P3 п.4B | P1 ↔ P3 | Streaming pipeline: P3 переключает shell с blocking на push-tokenizer. Совместный merge `streaming-go-live`. |
| После P3 п.4A | P3 ↔ P1 | DOM bindings: добавляем wrapper hooks в `lumen-dom` (P1 ревьюит). |

Все остальные стыковки — drop-in (пустой stub → реальный impl, потребитель ничего не правит).

---

### Crate ownership matrix (защита от merge-конфликтов)

| Крейт / модуль | Единственный владелец | Дополняют (PR с согласованием) |
|---|---|---|
| `lumen-html-parser`, `lumen-css-parser`, `lumen-dom`, `lumen-layout`, `lumen-encoding` | P1 | P3 — wrapper hooks для GC (по согласованию) |
| `lumen-paint`, `lumen-font`, `lumen-image` | P2 | — |
| `lumen-network`, `lumen-storage`, `lumen-knowledge`, `lumen-shell`, `lumen-js`, `lumen-ai` | P3 | — |
| `lumen-driver`, `lumen-mcp-server`, `lumen-bidi-server`, `lumen-cdp-shim` (когда появится) | P3 | P1 — accessor expose из `lumen-layout` / `lumen-a11y` (по согласованию); P2 — `Renderer::render_to_image()` off-screen path (по согласованию) |
| `lumen-a11y` | P1 | P3 — читает snapshot для `BrowserSession::a11y_tree()` |
| `lumen-core::ext` | P3 | P1/P2 — добавляют trait если им нужно (sole-author commit, post-factum review) |
| `lumen-paint::display_list` (`DisplayCommand` enum) | P2 | P1 — добавляет варианты для новых layout-фич (например, для Grid) — P2 ревьюит |
| `samples/page.html`, snapshot tests | Кто меняет — тот и трогает | — |
| `graphic_tests/snapshots/*.png` (Уровень 3 эталоны, §15) | Кто реализует свойство — тот и обновляет (через `cargo test --update-snapshots`) | — |
| `lumen-shell::tab_lifecycle`, `lumen-storage::tab_snapshot`, `lumen-bench::memory` + `tier_transitions` | P3 (трек 10, ADR-008) | P1 — DOM arena serialization (10B); P2 — pure paint audit (10D.2), GPU layer cache (10F); P1+P2 — glyph atlas eviction (10G) |
| `lumen-core::ext::MemoryPressureSource` (новый trait-anchor, ADR-008 task 10H) | P3 | OS-specific impls: win32/linux/macos в одном крейте |

**Главное правило:** новый PR трогает крейты **только одного владельца**, кроме редко согласованных кросс-крейтных стыковок выше.

---

### Provisional крейты к включению по приоритету

| Крейт | Программист | Когда брать | Trait-anchor (Sprint 0) |
|---|---|---|---|
| ✅ `brotli-decompressor` | P3 (готово) | Подключён через `BrotliContentDecoder` в `lumen-network` (merge network-brotli, 2026-05-15) | расширение `ContentDecoder` |
| ✅ `psl` (выбран вместо `publicsuffix` — compiled-in) | P3 (готово) | Подключён через `PslProvider` в `lumen-storage` (merge network-publicsuffix, 2026-05-15) | `PublicSuffixList` |
| `idna` (полный UTS#46) | P3 (по факту edge case) | Когда найдём реальный кейс, не покрытый Punycode | `IdnaProvider` |
| `icu4x.segmenter` + `icu4x.linebreak` | P1 п.5 | После CSS типизации (1B) | `UnicodeProvider` |
| `ruzstd` / `zstd-safe` | P3 (после brotli) | После Brotli — дёшево добавить | расширение `ContentDecoder` |
| `image-webp`, `zune-jpeg` | P2 (опционально) | Если P2 не загружен Phase 2 работой | `ImageDecoder` |
| `woff2` | P2 (Phase 2) | При WebFonts | `FontFormat` |
| `hyphenation` | P1 (Phase 2-3) | При типографике | `HyphenationProvider` |
| `hunspell-rs` / `spellbook` | P3 (Phase 3) | При spell-check | `SpellChecker` |
| `quinn` (HTTP/3) | P3 (никогда в обозримом) | — | расширение `NetworkTransport` |
| `tiny-skia` (CPU rasterizer, **только под `cfg(test)`**) | P2 (Phase 0, opt-in) | Для in-process pixel snapshot tests (§15 уровень 3) — детерминизм между Windows/macOS/Linux CI. Не попадает в release-бинарь. | расширение `Renderer` (off-screen CPU path) |

---

### Browser fundamentals (справочный список, с учётом новой раскладки)

Список упущений, обнаруженных при сравнении с реальными движками 2026-05-14. Все маркеры обновлены под P1-P3. Каждый пункт **критичен для функциональности** при достижении соответствующей фазы — без них браузер не работает как браузер, а не Phase-0-демо.

#### Phase 1 (Reader) — добавить к существующему scope

- **`[P3]` HTML event loop + microtasks + rendering steps + observers.** 🟡 **Framework + integration в winit-loop + task source priorities + requestIdleCallback + `run_idle_callbacks` в about_to_wait готовы** (`lumen-shell::runtime` + Lumen-handlers — см. [SUBSYSTEMS.md](SUBSYSTEMS.md) → `lumen-shell`). about_to_wait → step()×N (cap 256) → run_idle_callbacks(`IDLE_BUDGET_MS=10.0` если дошли до Idle, 0 ms иначе), Resized → deliver_observer_records(Resize), RedrawRequested → run_rendering_step(timestamp_ms) перед render(); `TaskQueue` обходит `TaskSource::PRIORITY_ORDER` на pop. **`setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` + `scheduler.postTask`/`yield` реализованы как async JS timer queue**: `_lumen_tick_timers()` дрейнируется в about_to_wait, `_lumen_request_wakeup(deadline_ms)` уведомляет shell о следующем таймере → `ControlFlow::WaitUntil` (2026-05-25). **Layout invalidation after JS DOM mutations**: `dom_dirty: Arc<AtomicBool>` в `QuickJsRuntime` — проставляется всеми мутирующими биндингами (`set_attr`/`set_text_content`/`set_inner_html`/`append_child`/`remove_child`/`remove_attr`); `RedrawRequested` шаг 6 вызывает `relayout()` если флаг взведён (2026-05-25). **`MutationObserver` / `ResizeObserver` / `IntersectionObserver` + `getBoundingClientRect` готовы** (2026-05-26): JS-классы в `crates/js/src/dom.rs`, `_lumen_get_bounding_rect` / `_lumen_get_viewport_size` Rust-биндинги через `Arc<Mutex<HashMap<u32,[f32;4]>>>`, deliver вызывается из shell после каждого `relayout_page`. 200 тестов lumen-js. **Осталось:** правильный ordering rendering steps stage (style → layout → paint как cascade), PerformanceObserver.
- **`[P1+P2]` Stacking contexts + правильный CSS Painting Order** (CSS 2.1 Appendix E). 7-уровневый порядок paint (background → border → block descendants → floats → non-positioned inline → positioned по z-index, рекурсивно). **P1** — модель stacking-ов и order computation в layout; **P2** — paint-side traversal в правильном порядке.
- **`[P2+P1]` Compositor thread + property trees.** Отдельные TransformTree / ScrollTree / EffectTree / ClipTree, копируются на compositor thread. Двухбуферная commit-модель. Off-main-thread scroll. **P2** — compositor pipeline / GPU primitives / layer tree; **P1 — готово (`PropertyTrees::build`, `Mat4` builders + multiply, ветка `property-trees-build`)** — построение четырёх деревьев из style/layout с учётом transform-origin.
- ✅ **`[P2]` Stacking-aware hit testing.** Реализовано в `lumen-paint::hit_test` (см. § «P2 2B» и [SUBSYSTEMS.md](SUBSYSTEMS.md) → `lumen-paint`).
- ✅ **`[P1]` Quirks mode vs standards mode — application в layout/cascade.** Полностью: table UA-reset, line-height replaced, unitless length, hashless hex color, table cell width hint, §3.5 html `height:100vh` (viewport basis). 2026-05-24.
- 🟡 **`[P3]` Same-Origin Policy enforcement + CORS preflight.** `Origin` tuple реализован (`lumen-network::Origin`, HTML LS §7.5 — scheme/host/port + same_origin + is_potentially_trustworthy). **CORS preflight enforcement реализован полностью** (`HttpClient::with_cors_cache` + `fetch_cors(CorsRequest)` — OPTIONS preflight + cache + actual-response validation per hop, ветка `cors-preflight-enforcement`). SOP checks при postMessage / storage / cookies — следующая задача (применить classifier в shell). Phase 0 ограничения CORS: request body для POST/PUT (HttpClient GET-only), cookie-jar integration.
- 🟡 **`[P3]` Mixed-content blocking + `<iframe sandbox>`.** Classifier-ы реализованы (`lumen-network::classify_subresource_request` для blockable/optionally + `SandboxFlags`/`parse_sandbox_value` для всех 14 keyword-ов). Остаётся: enforcement в HttpClient (блочить blockable до TCP) + DOM-применение sandbox в shell.
- **`[P1+P3]` Preload scanner.** ✅ **Готово полностью.** P1: `lumen_html_parser::preload_scanner::scan_preload_hints`. P3: интегрирован в shell streaming pipeline — `LoadEvent::EarlyPreloadHints` эмитируется из background-потока ДО первого `HtmlChunk` (скан первых STREAM_CHUNK_BYTES байт, обычно весь `<head>`). `dispatch_preload_hints` принимает `seen: &mut HashSet<String>` для cross-call дедупликации. Phase 0: prefetch блокирующий; параллельный fetch — будущая задача.

#### Phase 2 (Interactive) — без этого современный веб не функционален

- 🟡 **`[P1+P3]` Shadow DOM + custom elements + `<template>` + `<slot>`.** **P1 done** — `ShadowRootMode`, `NodeData::ShadowRoot`, `Document::attach_shadow/shadow_root_of/is_shadow_host`, `FlatTree` + `build_flat_tree` (slot assignment, fallback content, zero-alloc fast path), layout wiring (`build_box`/`collect_inline_segments` use `flat.children_of`). **P3** — JS bindings (`Element.attachShadow`, `customElements.define`) + lifecycle dispatch. **P4** — `:host`, `::slotted` CSS pseudo-classes (marked with `// CSS: :host, ::slotted` in `build_box`).
- **`[P1+P3]` Accessibility tree + platform bridges.** **P1** — построение accessibility tree из DOM/layout + ARIA semantics + focus model; **P3** — platform bridges (UIA / AT-SPI / NSAccessibility) + focus dispatch.
- 🟡 **`[P1+P3]` Forms runtime.** **P1 done** — `ValidityState` + все флаги HTML5 §4.10.21.1 в `lumen-dom::element_validity`, `check_validity_form`/`invalid_controls_in_form`, `build_form_submit` блокирует submit при невалидных контролах. Validation pseudo-classes (`:valid`/`:invalid`/`:required` и т.д.) готовы ранее. **P3** — native pickers, autofill popup, validation tooltip UI.
- ✅ **`[P1+P2]` `<picture>` / `srcset` / `sizes` + `loading="lazy"`.** **P1**: готовы parser + pickers + `pick_picture_source` + L4 nested + IntersectionObserver event source для lazy (rootMargin 1-viewport-ahead, через `_lazy_io` внутри `_lumen_init_lazy_images`). **P2** — image-side GPU upload + integration в shell.
- **`[P3]` IME composition events** (`compositionstart` / `update` / `end`, `KeyboardEvent.isComposing`). Интегрируется через winit IME API + DOM events.
- **`[P3]` Range requests — готово** (single + multi + suffix + If-Range). `fetch_range` + `fetch_multi_range` + `parse_multipart_byteranges` в lumen-network — нормализуют все формы 206/multipart-ответов в типизированные `Range/MultiRangeResponse`. Осталось: shell-интеграция под resume downloads и `<video>` seek по таблице сегментов. (Brotli — **готово**: `BrotliContentDecoder` за `ContentDecoder` в `lumen-network`.)
- **`[P3]` DevTools / Inspector минимум.** DOM tree view + computed styles panel + network log. Стандарт — Chrome DevTools Protocol (CDP) как WebSocket-сервер.
- **`[P1+P2]` `mix-blend-mode` / `background-blend-mode` / `isolation` / `backdrop-filter`.** 16 blend modes; `backdrop-filter` — отдельный pass blur на снапшоте под элементом.

#### Phase 3+ — без этого браузер не полнофункциональный

- ✅ **`[P1]` Page Visibility API + document.readyState + navigator.sendBeacon.** `document.visibilityState`/`document.hidden` (W3C Page Visibility Level 2) — always `"visible"` in single-window Lumen; `visibilitychange` event on focus/blur via `_lumen_apply_visibility(hidden)`. `document.readyState` (HTML LS §8.2.3) — `"loading"` → `"interactive"` (after HTML parse + inline scripts; `readystatechange` + `DOMContentLoaded` dispatched) → `"complete"` (after all resources; `readystatechange` + `window.load` dispatched); `_lumen_apply_ready_state` JS driver; shell calls `notify_dom_content_loaded()` / `notify_window_loaded()` at correct lifecycle points. `navigator.sendBeacon(url, data)` (Beacon API §3.1) — `_lumen_send_beacon` native binding fires synchronous POST via `JsFetchProvider`; accepts string/URLSearchParams/FormData/Blob data. `window.addEventListener('load'/'DOMContentLoaded'/'visibilitychange')` + `window.onload` + fast-path for late listeners. `document.dispatchEvent` now actually dispatches to document-level listeners (was a no-op stub). 12 новых тестов (416 итого lumen-js). 2026-05-31.
- ✅ **`[P1]` Web Crypto API + structuredClone (W3C Web Cryptography API §3, HTML LS §2.7).** `window.crypto.getRandomValues(typedArray)` fills typed array from OS CSPRNG (`getrandom`); `window.crypto.randomUUID()` generates RFC 4122 v4 UUID. `window.crypto.subtle.digest(algo, data)` — SHA-1/256/384/512 via Rust `sha2`+`sha1` crates, returns Promise<ArrayBuffer>. `structuredClone(val)` — deep clone of primitives / Object / Array / Date / RegExp; `window.structuredClone` alias. 15 новых тестов (385 итого lumen-js). 2026-05-31.
- ✅ **`[P3]` Web Storage API (`localStorage` + `sessionStorage`).** `WebStorage` in `lumen-core::web_storage` (insertion-order key list, `get_item/set_item/remove_item/clear/key/len`). `_lumen_ls_*` + `_lumen_ss_*` native bindings in `lumen-js`. `_lumen_make_storage` factory + `localStorage`/`sessionStorage` globals in `WEB_API_SHIM`. Shell stores `HashMap<origin, Arc<Mutex<WebStorage>>>` per session; `ls_store_for_base` extracts SOP-partitioned store from `ResourceBase`; `sessionStorage` is fresh per page load. 8 tests. Phase 0: in-memory only (no disk persistence).
- ✅ **`[P1]` IndexedDB (Indexed Database API 3.0).** Pure-JS in-memory implementation in `WEB_API_SHIM` (`crates/js/src/dom.rs`): `indexedDB` (`open`/`deleteDatabase`/`databases`/`cmp`), `IDBDatabase`/`IDBTransaction`/`IDBObjectStore`/`IDBIndex`/`IDBCursor`/`IDBKeyRange`/`IDBRequest`/`IDBOpenDBRequest`. CRUD + indexes (unique/multiEntry) + cursors (forward/reverse/unique) + key ranges; key ordering number<date<string<array; dotted/array key paths; autoIncrement. Deferred-execution model: request actions run at dispatch time in FIFO order, `_lumen_idb_flush()` delivers events (queueMicrotask + shell tick). 23 tests. **Persistence:** Rust-backed via `IdbBackend` trait (`lumen-core::ext`) → `IdbStore` over `StorageBackend` (`lumen-storage`): the shim serializes all per-origin databases into one tagged-JSON snapshot (Dates preserved), persisted after each mutating flush (`_lumen_idb_persist`) and restored on init (`_lumen_idb_load`); databases survive page reload. Shell wires an `InMemoryStorage` backend (process lifetime, mirrors `localStorage`); disk durability is a one-line swap to `SqliteStorage`.
- ✅ **`[P3]` Persistent JS runtime + event bubbling.** `LayoutSource::document` и `ParsedPage::document` → `Arc<Mutex<Document>>`. `run_scripts_with_dom` возвращает живой `Option<Box<dyn PersistentJs>>` — рантайм не уничтожается после начальных скриптов. `Lumen::js_ctx` хранит контекст пока страница открыта. Клики диспатчируются через `_lumen_dispatch_bubble(nid,'click')` в JS — обход предков + document-level listeners. `document.addEventListener/removeEventListener` работают через sentinel NID=-1. `Event.cancelBubble` + `stopPropagation` + JS-triggered navigation после клика.
- ✅ **`[P3]` WebSockets (RFC 6455) + Server-Sent Events + Fetch API runtime.** ✅ WS: RFC 6455 upgrade + frame codec + JS API (`WebSocket` class, `JsWebSocketProvider`/`JsWsEvent`/`JsWebSocketSession` traits, background recv thread, `_lumen_pump_websockets()`, 12 тестов). ✅ SSE: `SseParser` + `EventSource` client + `EventSource` JS stub. ✅ Fetch: `fetch()` / `Request` / `Response` / `Headers` / `AbortController` / `AbortSignal` в JS shim; `JsFetchProvider` trait; `HttpClient` реализует.
- **`[P3]` HTTP auth — Basic + Digest готовы** (см. status). **Осталось:** Negotiate/NTLM, client certificates mTLS, UI-popup для credentials.
- **`[P3]` OCSP stapling + CT log enforcement + invalid cert UI.**
- **`[P3]` Safe Browsing — готово** (см. status). Отложено: 4-byte prefixes с full-hash callback, public-suffix list для безопасной обрезки host-suffixes ниже eTLD+1.
- **`[P3]` Back/forward cache (bfcache).** Снапшот DOM+JS heap для мгновенного back. Eligibility rules.
- **`[P3]` Navigation API + History API runtime.** ✅ `history.pushState/replaceState/go/back/forward` + `popstate` event + `location` object (href/protocol/hostname/host/port/pathname/search/hash/origin) инициализируется из page URL, `location.href=` / `assign()` / `replace()` / `reload()` → навигация через shell. Отложено: `navigate` event (Navigation API 2023).
- **`[P1+P2+P3]` Web Animations API runtime** поверх parsed `@keyframes` / transitions. **P1** — интерполяция; **P2** — compositor offload для transform / opacity; **P3** — animation timeline scheduling в rendering steps stage.
- **`[P1+P3]` `<contenteditable>` + Input Events Level 2 + Selection / Range API.** **P1** — DOM mutations + Selection / Range типы + `beforeinput` / `input`; **P3** — input dispatch (key + IME + drag-drop + paste), undo/redo stack в shell.
- **`[P3]` Service Worker runtime.** Fetch interception, push delivery, background sync, cache strategies. **P3** — и backend (fetch hook + storage), и JS worker context + lifecycle + `clients` API (бывший P4 объединён).
- **`[P3]` Spell check** через **provisional `hunspell-rs`** за `SpellChecker` от Sprint 0. Squiggly underline в render, context menu suggestions. Русский словарь — часть «русский first-class».
- **✅ `[P2]` Variable fonts axes runtime.** `font-variation-settings` CSS Fonts L4 §7 — cascade в lumen-layout (`FontVariationSetting { tag: [u8;4], value: f32 }`, inherited, `parse_font_variation_settings`), DrawText.font_variation_axes, normalization через fvar+avar в renderer.
- **`[P2]` Color management + Display P3 / Rec2020 / ICC profiles.** Для `<canvas>` / `<img>` / CSS `color()` функций (CSS Color L4).
- **`[P1+P2+P3]` Print pipeline runtime.** **P1** — pagination algorithm; **P2** — PDF rendering из display list; **P3** — print preview UI.
- 🟡 **`[P1+P3]` GC integration JS ↔ DOM (cycle collector).** **P1 done** — `Document::acquire_js_ref(NodeId) -> u32` / `release_js_ref` / `js_ref_count` / `is_detached` / `dead_node_ids()` в `lumen-dom`. `js_refs: HashMap<NodeId,u32>` (serde skip — не сериализуется при гибернации). 11 unit-тестов. **P3** — QuickJS finalizer callback вызывает `release_js_ref` + idle GC tick дренирует `dead_node_ids()` в shell.
- **`[P3]` Permission prompt UI + Download UI.**
- **`[P3]` GPU process / sandbox.** Реальный browser-grade sandbox: seccomp (Linux), AppContainer (Windows), App Sandbox (macOS), GPU процесс отдельно от renderer-а.

### Не приоритет, держим в голове

- **`[P2]`** Variable fonts parsers (fvar/gvar/avar/HVAR/VVAR/MVAR) в `lumen-font` — реализовано. **IUP + apply_glyph_deltas в `lumen-font::variation`** реализовано (ветка `variable-fonts-runtime`): `apply_variations_to_simple_outline(contours, variations, coords)` применяет gvar deltas к outline-контурам in-place с OpenType-spec IUP для untouched точек, 19 unit-тестов. **`Font::glyph_resolved_with_coords` (ветка `variable-fonts-glyph-resolved`)**: variable-fonts entry поверх resolve-pipeline (simple-outline + composite recursion, gvar парсится один раз перед спуском); пустой coords / font без gvar — short-circuit на `glyph_resolved`; component-level gvar variations отложены. 6 integration на synthetic-TTF + 2 на Inter (без gvar). **Atlas variation-aware cache key (ветка `atlas-variation-key`)**: `AtlasKey { face_id, glyph_id, size_bin, coords_hash: u64 }` через `AtlasKey::hash_coords(&[f32])` — variant glyph не перезаписывает base. `DisplayCommand::DrawText.font_variation_coords: Vec<f32>` — interface-first hook для P1 cascade (empty = default-instance, snapshot-тесты неизменны). Renderer зовёт `glyph_resolved_with_coords` и передаёт coords. Осталось: CSS `font-variation-settings` cascade в `lumen-layout` (parser longhand `font-variation-settings: "wght" 600, "wdth" 80`, computed-value-time clamp через `fvar.axes` + `avar.normalize`, заполнение DrawText.font_variation_coords).
- **`[P2]`** GSUB/GPOS shaping (для арабского, индийского, тайского). Текущая позиция — добавим как exception #5 (rustybuzz) или сами для базовых случаев.
- ✅ **`[P1]`** ADR-инфраструктура (`docs/decisions/`) — формализация decisions log. Реализовано 2026-05-25: TEMPLATE.md + README-индекс + ADR-001..005 (custom engine, dep policy, SQLite, JS runtime, image decoding).
- **`[P3]`** StorageBackend trait: добавить origin partitioning параметр (`(origin, top_level_site)`) ДО первой реализации, чтобы не переделывать.
- Composite glyphs с ARGS_ARE_XY_VALUES=0 (point alignment) — реализовано — см. `git log --oneline | grep composite-point-align`.
- CSS4 pseudo-class `:has(...)` — реализовано — см. `git log --oneline | grep css-has-pseudo`.

---

## История реализации

Подробные notes о том, что было реализовано в каждой ветке. Обновляется при merge.

---

### P1 — Frontend engine

#### 1A — Quirks-mode application

- **`quirks-mode-application`**: `apply_quirks_table_reset` в `compute_style` — сбрасывает font/color/text-align/white-space у `<table>` в Quirks-mode к initial-values (эквивалент UA-stylesheet rule в Chromium/Firefox/WebKit). Author CSS поверх — выигрывает.
- **`quirks-hashless-hex-color`** (CSS Quirks Mode §3.4): `parse_color_legacy(s, is_quirks)` — в Quirks-mode bare hex digits длиной 3/6/8 без `#` парсятся как color. Применяется ко всем CSS `<color>`-полям через cascade pipeline. В Standards/LimitedQuirks — отвергает hashless форму.
- **`html-legacy-bgcolor`** (HTML5 §2.4.6 + §15): `parse_legacy_color_html_attr` — лояльный парсер для presentational-hint атрибутов (named colors, `#rgb`/`#rrggbb`, hashless hex через padding/truncate, non-BMP→«00», unhex→«0»). `apply_bgcolor_presentational_hint` для `<body>`/`<table>`/`<thead>`/`<tbody>`/`<tfoot>`/`<tr>`/`<td>`/`<th>` ДО CSS-каскада. 16 unit-тестов + 8 integration.
- **`html-legacy-text-color`** (HTML5 §15.3.6 + §15.3.2): `apply_text_color_presentational_hint` — `<body text="…">` → `body.color`; `<font color="…">` → `color`. Тот же `parse_legacy_color_html_attr`. 12 unit-тестов. `<body link/vlink/alink>` отложены (требуют UA-rules с descendant-selector + `:visited`/`:active`).
- **`canvas-bg-propagation`** (CSS Backgrounds L3 §2.11.2): `propagate_canvas_background(doc, &mut root)` в `lumen-layout::box_tree` между `build_box` и `lay_out`. Если у `<html>` нет фона, фон `<body>` (`background-color` + `background-image`) переносится на html-box, у body обнуляется. 6 unit-тестов. SVG/MathML root-ы и остальные 6 `background-*` longhand-ов не propagated.

- **`unitless-length-quirk`** (CSS Quirks Mode §3.3): `parse_length_q(s, is_quirks)` — в quirks-mode unitless non-zero число принимается как px; в standards-mode отклоняется (кроме `0`). `parse_length(s)` → `parse_length_q(s, true)` для обратной совместимости. Все вызовы в каскаде (`width`/`height`/`margin`/`padding`/`border-*-width`/`border-radius`/`font-size`/`text-indent`/`letter-spacing`/`word-spacing`/`outline-offset`/`vertical-align`) обновлены. 4 unit-теста.

- **`ie7-line-height-quirk`** (CSS Quirks Mode §3.2): `apply_quirks_line_height` в `compute_style` — в quirks-mode replaced-элементы (`img`/`video`/`canvas`/`embed`/`object`/`iframe`/`input`/`textarea`/`select`/`audio`) получают UA-правило `line-height: 1`, блокирующее наследование «normal» и зазор под `<img>` (как IE7). Author CSS поверх — выигрывает. 5 unit-тестов + обновлён snapshot `img_replaced_element`.

- **`table-cell-width-quirk`** (CSS Quirks Mode §4.1 + HTML5 §14.3.9): `apply_table_cell_width_hint` + `parse_html_length_attr` — `<td>/<th> width="N"` в quirks-mode → `min-width: Npx` (не width; ячейка не сужается ниже указанного, но может расшириться); в standards-mode → `width: Npx`. `<table> width="N"` → `width: Npx` в обоих режимах. `height` attr на td/th/table → `height: Npx` без quirks-варианта. Поддержка `%`-значений (`Length::Percent`). 10 unit-тестов.

#### 2A — Stacking contexts impl

**`stacking-contexts-build`**: `StackingTree::build` + 4 новых свойства в ComputedStyle. Стекинг-контексты создают: `opacity<1`, `transform != none`, `filter`, `clip-path`, `isolation: isolate`, `mix-blend-mode != normal`, `will-change` с stacking-property, `position: fixed/sticky` всегда, `position: relative/absolute` с явным z-index. Flex/grid item с z-index — отложено до реального flex/grid pass.

#### 2B — Property trees построение

**`property-trees-build`**: `PropertyTrees::build(&LayoutBox)` обходит layout pre-order и строит 4 дерева. Триггеры: TransformNode — `transform != []` (с локальной матрицей через transform-origin); ScrollNode — `overflow-x/y != visible`; EffectNode — `opacity<1` ∨ `filter != []` ∨ `mix-blend-mode != normal` ∨ `isolation: isolate`; ClipNode — `clip-path` ∨ `overflow-x/y` clipping. Mat4 расширен 2D-builders (translation/scale/rotate/skewX/skewY/matrix) + column-major multiply. Анонимные InlineRun-ы пропускаются.

#### 3A — Web Animations interpolation

- **`animation-longhands`**: `TimingFunction` enum (Linear/CubicBezier/Steps) + parser; `animation-*` longhands в ComputedStyle (9 longhands — comma-list, не наследуются); `transition_timing_functions`.
- **`animation-interpolator`**: `TimingFunction::progress(t)` (Newton-Raphson + bisection fallback / Steps 4 step-position); `LinearInterpolator` — drop-in замена `NoopInterpolator` для Number/Length/Color, fallback to step-half для Discrete/mixed-unit/Calc.
- **`transform-interpolate`** (CSS Transforms L2 §15): `AnimValue::TransformList` + два пути: matched-pair lerp / 2D matrix decompose fallback (tx/ty/scale_x/scale_y/skew/rotation с shortest-path). 24 теста.
- **`animation-shorthand`** (CSS Animations L1 §4): `apply_animation_shorthand` парсит comma-list `<single-animation>#`; `tokenize_with_parens` для cubic-bezier/steps-аргументов. 28 unit-тестов.
- **`transition-shorthand`** (CSS Transitions L1 §3): `apply_transition_shorthand`, реиспользует утилиты из animation-shorthand. 17 unit-тестов.
- **`filter-list-interpolate`** (CSS Filter Effects L1 §6): `AnimValue::FilterList` + `interpolate_filter_list` с identity-padding для коротких списков. 15 unit-тестов.
- **`gradient-stops-interpolate`** (CSS Images L3 §3.5.1): `GradientStop { color, position }` + `interpolate_gradient_stops` (pairwise lerp цвета sRGB + позиции). 11 unit-тестов.

- **`gradient-stops-parser`** (CSS Images L3 §3.3): `pub fn parse_gradient_stops(s: &str) -> Vec<GradientStop>` + `paren_whitespace_tokens` helper — извлекает color stops из любой gradient-строки (linear/radial/conic + repeating). Стратегия: top-level comma split → paren-aware whitespace tokenize → первый token без `parse_color` пропускается (direction/hint); два позиции у одного stop → expand в два stop. 13 unit-тестов (итого 170 тестов на animation/transition+gradient).

Итого: 170 тестов. **P1 часть 3A завершена.** Осталось (другие разработчики): integration в scheduling (P3), compositor offload (P2).

#### 3B — Push-tokenizer + incremental tree builder

**`push-tokenizer`**: `PushTokenizer { feed, feed_bytes, end }` поверх pull-Tokenizer-а с `find_safe_split`-эвристикой (учитывает `<!--…-->`, `<!DOCTYPE…>`, `<tag…>`, `&entity;`); `IncrementalTreeBuilder { feed, finish }` с text-node coalescing. Инвариант: pull/push дают побайтово равный `Document`. `feed_bytes(&[u8])` буферизует незавершённые UTF-8 последовательности на границах chunk-ов (partial_utf8 до 3 байт); явно невалидные байты → U+FFFD inline; незавершённая последовательность при `end()` → U+FFFD (WHATWG Encoding §4). 342 теста (36 push_tokenizer + 15 incremental_tree_builder + остальные). **Разблокировано:** P3 п.4B (streaming pipeline) — shell переключается с blocking на push/bytes-режим.

#### UA stylesheet phase 1 (2026-05-20)

**`ua-stylesheet-phase1`**: `apply_ua_text_decoration` (del/s → line-through, ins/u/a[href] → underline), `ua_link_color` (a[href] → #0000ee), `ua_font_size_factor` (small/sub/sup → 0.83 × parent), `ua_vertical_align` (sub → Sub, sup → Super). `default_display` расширен: del/ins/s → Inline. 7 unit-тестов. Исправляет BUG-007/008/009/012.

#### 4A — `<picture>`/`srcset`/`sizes` finishing

- **`media-query-nested-not`**: `MediaClause::Nested` + paren-aware split — L4 nested `(not (...))` / `((...))` в media-condition.
- **`picture-srcset-integration`**: `lumen-layout::box_tree::resolve_image_source(doc, img_id, viewport)` — вызывает `pick_picture_source`/`pick_img_source` с DPR=1.0; intrinsic-dims из `<source>` как presentational hint; `<source>`/`<track>` → `Display::None`. 9 unit-тестов.

- **`picture-shell-4a`** (P2): `lumen_layout::collect_image_requests(doc, viewport) -> Vec<ImageRequest>` — публичная функция, использует `resolve_image_source` для каждого `<img>`. `fetch_and_decode_images` в shell принимает viewport и вызывает `collect_image_requests` вместо `collect_img_entries` — ключи в `register_image` совпадают с `DrawImage.src` для srcset/picture. 7 unit-тестов.

**Готово (2026-05-29):** IntersectionObserver event source для `loading="lazy"` — `_lumen_init_lazy_images` создаёт внутренний IO с rootMargin 1-viewport-height; `_parse_root_margin` + rootMargin-aware intersection; `_lumen_deliver_lazy_images` → no-op.

#### 4B.6-7 — CSS Grid layout (2026-05-20)

- **`grid-properties`**: `GridTrackSize` (Auto/Length/Fr/MinContent/MaxContent/Minmax), `GridLine` (Auto/Line(i32)/Span(u32)), `GridAutoFlow` (Row/Column/RowDense/ColumnDense). `ComputedStyle` расширен 9 полями: `grid_template_columns`, `grid_template_rows`, `grid_auto_flow`, `grid_auto_columns`, `grid_auto_rows`, `grid_column_start`, `grid_column_end`, `grid_row_start`, `grid_row_end`.
- **Парсеры**: `GridTrackSize::parse_track_list` — whitespace-tokenize с paren-aware depth для `minmax()/repeat()/fit-content()`; `repeat(N, ...)` разворачивается inline; `<Nfr>` fr-units. `GridLine::parse` — auto/span/integer. Shorthand-ы: `grid-column`, `grid-row`, `grid-area` (4-компонентный row/col/row-end/col-end), `grid-template` (slash-split), `grid` (alias к template).
- **`lay_out_grid`**: Phase-0 реализация CSS Grid L1 §12. Явная расстановка (integer line-числа + span N), auto-placement (row и column flow, scan-forward без dense). Fr-раздача (`free_space / total_fr`). Auto-строки — по max-content детей. align-items / justify-items внутри ячеек. gap (column-gap / row-gap).
- **blockify**: прямые дети grid/flex-контейнеров собираются индивидуальными `build_box()` без обёртки в `InlineRun` — CSS Grid L1 §6 «Item Blockification».
- 12 unit-тестов. Итого: 1367 тестов.

#### 4C — CSS Positioned Layout (2026-05-20)

- **`top/right/bottom/left`**: 4 новых поля `LengthOrAuto` в `ComputedStyle`, initial = Auto, non-inherited. Парсинг через существующий `set_margin_side`. **`inset` shorthand** — 4-value box-shorthand (1/2/3/4 значения → top/right/bottom/left по CSS box-shorthand grammars). CSS-wide keywords (`inherit/initial/unset/revert`) для всех 5 свойств.
- **`position: relative`**: `shift_tree(b, dx, dy)` рекурсивно сдвигает бокс и всё поддерево. После normal-flow layout вычисляются `off_x` (left ?? -right ?? 0) и `off_y` (top ?? -bottom ?? 0) через resolve %. Нормальный поток не пересчитывается (shift — visual-only).
- **`position: absolute/fixed`**: абсолютные потомки собираются в `abs_deferred: Vec<(usize, static_x, static_y)>` при обходе normal-flow children (skip из потока). После нормального layout вызывается `lay_out_abs_children` — двухпроходный алгоритм: layout при (0,0) для получения размеров, затем вычисление desired position по PCB + shift_tree. Fixed — CB = viewport; absolute — CB = ближайший positioned ancestor (Positioned Containing Block, `pcb: Rect` передаётся рекурсивно).
- **`pcb: Rect` в lay_out/lay_out_flex/lay_out_grid**: root получает `Rect(0,0,vw,vh)`, на каждом non-static элементе PCB обновляется после layout (финальный rect с high и шириной).
- 9 unit-тестов (relative offset/bottom-right, absolute top-left/bottom-right/viewport-CB/out-of-flow, fixed, inset 4-value, relative all-auto). Итого: 1376 тестов.

---

### P2 — Backend rendering

#### 1A — Font fallback/matcher

(a) picker `match_face` по CSS Fonts L4 §5.2 + OS/2 парсер + `FaceRecord` (`lumen-font::os2`, `lumen-core::ext::match_face`); (b) `DisplayCommand::DrawText` несёт `font_family`/`font_weight`/`font_style`; (c) `Renderer` хранит `Vec<LoadedFace>` + `font_provider: Option<Arc<dyn FontProvider>>` (по умолчанию `SystemFontIndex`), `resolve_face_id` лениво грузит TTF; (d) per-char codepoint cascade в `push_text_glyphs` — если у primary face нет глифа, обходим loaded faces (CSS Fonts L4 §5.3). **Осталось:** eager preload курируемого fallback-списка (Noto Color Emoji/Noto CJK).

#### 1B — Compositor thread + layer tree

- **Scaffolding**: trait-ы `Layer`/`LayerTree`/`Compositor` + `BasicLayerTree::single_layer` + `InProcessCompositor`.
- **Two-buffer commit**: `commit` → pending; `flush_pending()` атомарно промотирует в active; `has_pending()`.
- **DisplayCommand layer-ops**: `PushClipRect/PopClip`, `PushOpacity/PopOpacity`, `PushBlendMode/PopBlendMode` + `BlendMode` enum (17 режимов).
- **`layer-ops-emission`**: для боксов с `opacity<1`/`mix-blend-mode != Normal`/`overflow != Visible` эмитятся парные Push/Pop. `box_can_own_stacking_context` отсекает анонимные InlineRun-ы.
- **`compositor-thread`**: `ThreadedCompositor` + `ThreadedCompositorHandle` на `Arc<Mutex<ThreadedState>>`. trait `Compositor` переведён на `Arc<dyn LayerTree + Send + Sync>`. Multi-thread тесты.
- **`layer-pipeline-clip`** (PushClipRect → wgpu scissor rect): renderer перешёл на ordered `Vec<DrawOp>`; `clip_stack: Vec<Rect>` (intersection с топом, CSS Masking L1 §3); DPR-aware. 18 unit-тестов.
- **`layer-pipeline-opacity`** (PushOpacity → alpha-multiply, Phase 0): `opacity_stack: Vec<f32>`; `effective_alpha` (product, clamp [0,1]). ImageVertex расширен Float32 `alpha` attribute. Phase 0: overlapping children alpha-blend попарно, не single-pass. 9 unit-тестов.

**Осталось:** compositor.active_tree() в shell (P3), реальный tick-loop с `JoinHandle`, PushBlendMode pipeline.

**1B.4 done (2026-05-19):** true single-pass off-screen opacity — render plan (DrawBatch/Composite), OffscreenLayer pool, COMPOSITE_SHADER, multi-pass wgpu pipeline.

#### 2A — Painting order traversal

`PaintOrder::from_tree(&StackingTree)` — рекурсивный обход CSS 2.1 Appendix E: `RootBackground → neg-z children fully → BlockBackgrounds/Floats/InlineContent → auto/0-z children → positive-z children`. `build_display_list_ordered(root, &StackingTree, &PaintOrder) -> DisplayList`: bucket-per-SC, child-SC в правильных слотах. Phase 0: BlockBackgrounds/Floats/InlineContent в одном bucket-е.

#### 2B — Stacking-aware hit testing

**`stacking-hit-testing`**: `lumen-paint::hit_test(point, &LayoutBox) -> Option<HitTestResult>` — обратный CSS Painting Order traversal с группами positive-z SC (desc по z)/in-flow+auto-0-z (reverse DOM)/negative-z SC (desc по z); фильтры `pointer-events: none`, `display: none`; transform inversion через `Mat4::invert_2d_affine()`. `HitTestResult.path` — ancestor chain. 14 unit-тестов + 9 на Mat4 invert. Phase 0: InlineRun → node = id родителя; только 2D affine.

#### 4 — mix-blend-mode GPU pipeline (2026-05-20)

**`blend-mode-pipeline-p2`**: CSS Compositing & Blending L1 §8 — 17 blend modes в wgpu.
- `BLEND_SHADER_SRC` (WGSL): два текстурных входа (`t_src` + `t_dst`), uniform u32 `blend_mode`; формула §8: `Co = αs·B(Cs,Cd) + αs·Cd·(1-αd) + Cd·(1-αs)`; 12 separable-режимов + 4 non-separable (Hue/Sat/Color/Lum).
- `OffscreenLayer.texture` — COPY\_SRC добавлен; `ensure_scratch_layer()` — COPY\_DST + TEXTURE\_BINDING.
- `CompositePlan::mode: BlendMode`; при non-Normal рендер-план создаёт отдельный уровень; Composite path: `copy_texture_to_texture` → write uniform → render blend\_pipeline.
- Деградация для level==1 (surface): нет COPY\_SRC на swapchain → fallback normal alpha-blend.
- Render planning: `PushBlendMode(non-Normal)` → новый уровень + `level_blend_mode_stack`; `PopBlendMode` → `Composite { mode }`; Normal → pass-through без offscreen.
- 3 новых теста, 253 total в lumen-paint.

---

#### 3A — Color management + Display P3/Rec2020 (2026-05-20)

**`color-management-p3`**: CSS Color L4 §10 wide-gamut pipeline.
- `ColorSpace` enum (`Srgb | DisplayP3 | Rec2020`) + `ComputedStyle::color_space` (inherited).
- `ColorFloat { r, g, b, a: f32, space }` — wide-gamut хранение; `to_srgb_color()` через ICC-матрицы; `to_linear_srgb()` для GPU.
- Конверсионные функции (CSS Color L4 §10.9): `p3_linear_to_srgb_linear`, `rec2020_linear_to_srgb_linear`, `srgb_gamma_decode`, `rec2020_gamma_decode`.
- `CssColor::Wide(ColorFloat)` — 3-й вариант; `to_color_opt()` + `resolve_linear()` методы.
- Парсер `parse_css_color_fn`: `color(display-p3 r g b / a)`, `color(srgb …)`, `color(rec2020 …)`. Unitless 0..1 и %.
- display_list: `background_color` обрабатывает `Wide` через `to_color_opt()` (не пропускает).
- 16 новых тестов, 1339 total.

---

### P3 — Runtime + system

#### 2A — SOP/CORS/mixed-content/sandbox

- **`network-security-base`**: `Origin` tuple (HTML LS §7.5), `classify_subresource_request` (W3C Mixed Content + Fetch §3.2.7), `SandboxFlags` u32-bitset + `parse_sandbox_value` (14 keyword-ов).
- **`mixed-content-enforcement`**: `MixedContentPolicy` + builder `HttpClient::with_mixed_content_policy` + `fetch_subresource(url, destination)`; classify после HSTS upgrade, до RequestFilter; `RequestBlocked { reason: "mixed-content: ..." }` per redirect-hop.
- **`cors-preflight`**: `lumen-network::cors` — pure-логика preflight classifier + cache.
- **`cors-preflight-enforcement`**: `HttpClient::with_cors_cache(Arc<PreflightCache>)` + `fetch_cors(CorsRequest)`. OPTIONS preflight через `fetch_single`, evaluate_preflight_response, cache.insert. Actual-response validation per hop. Phase 0: GET-only, без cookie-jar integration.

**Осталось:** sandbox-application в DOM-загрузчике shell-я.

#### 3A — DPR + scroll в shell

- **`shell-dpr-support`**: `Renderer.scale_factor: f64` + `set_scale_factor` для `ScaleFactorChanged`, viewport uniform делится на DPR.
- **`shell-scroll-state`**: `Lumen { scroll_y, content_height }` + `Renderer::render(content, overlay, scroll_y)`. MouseWheel (LineDelta 40px/PixelDelta/DPR), стрелки (40px), Page/Home/End. Clamp + reload сбрасывает.
- **`find-scroll-to-match`**: `find::scroll_to_match(match_rect, viewport_height, current_scroll) -> Option<f32>` (верхняя четверть, `None` если уже виден).
- **`scrollbar-overlay`**: `build_scrollbar_overlay` + `thumb_geometry` — 8px у правого края, MIN_THUMB_HEIGHT=24. 12 unit-тестов.
- **`scrollbar-drag`** + **`scrollbar-track-click`**: `classify_track_click` → `TrackClick { None|Thumb|Above|Below }`; `ScrollDrag::scroll_for`. 17 unit-тестов.
- **`smooth-scroll`**: `ScrollAnim { start_y, target_y, start_time_ms }` + `ease_out_cubic(t)`. Аддитивный repeat-input. 12 unit-тестов.
- **`scrollbar-cursor-feedback`**: `cursor_icon_for_hover(TrackClick, drag_active) -> CursorIcon`. 5 unit-тестов.

**Осталось:** relayout-on-resize, горизонтальный scroll, momentum.

#### 3B — HTML event loop

Framework + winit-integration + task source priorities + requestIdleCallback + `run_idle_callbacks` в about_to_wait готовы (`lumen-shell::runtime`). about_to_wait → step()×N (cap 256) → run_idle_callbacks(`IDLE_BUDGET_MS=10.0`); Resized → deliver_observer_records(Resize); RedrawRequested → run_rendering_step(timestamp_ms) перед render(); `TaskQueue` обходит `TaskSource::PRIORITY_ORDER` на pop.

**Осталось:** reload через queue_task, правильный ordering rendering steps stage (style→layout→paint), `scheduler.postTask`, PerformanceObserver.

#### 1B — rquickjs integration scaffold

Новый крейт `lumen-js` (Permanent #5 §5): реализует `JsRuntime` trait через `rquickjs` v0.11 (QuickJS).

- **`QuickJsRuntime`**: `eval(script)` → `JsResult<JsValue>`, `set_global`, `get_global`, `call_function`. Внутри `Mutex<Inner { _rt: Runtime, ctx: Context }>` для `Send+Sync`. `Function::call` в 0.11 требует фиксированных `IntoArgs`-кортежей — `call_function` использует eval-workaround через временный глобал `__lum_args__` и `fn.apply(null, __lum_args__)`.
- **Конвертация JsValue↔Value**: null/undefined, bool, int/float, string, array (рекурсивно), object (итерация через `props::<String, Value>()`).
- **Обработка ошибок**: `Error::Exception` → `ctx.catch()` + `.message` property fallback → string coerce → «JS exception».
- **shell**: feature `quickjs` → `make_js_runtime()` возвращает `Box<dyn JsRuntime>` с `QuickJsRuntime`; без feature — `NullJsRuntime`.
- **16 тестов**: eval (number/string/bool/null/array/object/error/syntax), set/get global, call_function (с/без args), round-trip array/bool, engine_name, Send+Sync.

#### 4B.6 — Streaming pipeline: feed_bytes + raw byte chunks (2026-05-25)

**`p3-streaming-feed-bytes`**: `IncrementalTreeBuilder::feed_bytes(&[u8])` — делегирует в `PushTokenizer::feed_bytes`, буферизует незавершённые UTF-8 code-point-ы. `LoadEvent::HtmlChunk(String)` → `LoadEvent::HtmlChunk(Vec<u8>)`. `start_streaming_load()` упрощён: убран `lumen_encoding::decode` в фоновом потоке и ручное выравнивание по code-point границам. Decode происходит один раз в финальном pipeline (`LoadDone`). 3 новых теста (`feed_bytes_ascii`, кириллица, emoji) → 341 итого в html-parser.

---

#### 5B — HTTP Range requests

- **Single + open-ended + suffix `bytes=-N`**: `fetch_range(url, RangeSpec, Option<RangeValidator>)` + `If-Range`.
- **`http-multi-range`**: `fetch_multi_range(url, &[RangeSpec], Option<RangeValidator>)` + `parse_multipart_byteranges` + `parse_boundary_from_content_type`. `MultiRangeResponse`/`RangePart`. Нормализует 200/206-single/206-multipart в единый `Vec<RangePart>`.

**Осталось:** shell-интеграция (resume downloads + `<video>` seek по таблице сегментов).

---

### P4 — CSS

#### CSS Intrinsic Sizing L3 (2026-05-24)

**`p4-intrinsic-sizing`**: `Length::MinContent` / `Length::MaxContent` / `Length::FitContent(Option<Box<Length>>)` в enum `Length`. `parse_sizing_length()` парсит intrinsic keywords (плюс `stretch`/`-webkit-fill-available`/`-moz-available` → `FitContent(None)`); подключён в `apply_declaration()` для `width`/`height`/`min-*`/`max-*`. `Length::is_intrinsic()` — флаг для layout. `max_content_outer_width()` и `min_content_outer_width()` в `box_tree.rs` — рекурсивный обход дерева; Phase-0: min-content = максимальное слово (split по пробелам), max-content = весь текст одной строкой. `fit-content(L)` = min(max-content, max(min-content, L)). Минимальная/максимальная ширина через intrinsic функции учитывается при clamping. Graphic test 43. 11 unit-тестов. CSS-SPECS.md #21 🟡→✅.

---


## 0. Терминология

- **Lumen** — кодовое и публичное имя проекта. Бинарь: `lumen`, конфиг: `~/.config/lumen/`, репозиторий: `lumen`.
- **Browser** — общий термин: конечное приложение (окно, вкладки, UI, настройки).
- **Engine (rendering engine, browser engine)** — то, что превращает HTML+CSS+JS в пиксели: парсеры, DOM, layout, paint, compositor. Примеры: Blink, WebKit, Gecko, Servo, Ladybird.
- **JS engine** — отдельная подсистема, исполняющая JavaScript: V8, SpiderMonkey, JavaScriptCore, QuickJS.
- **WebView** — системный встраиваемый компонент (WebView2, WKWebView, WebKitGTK). Использует чужой движок.
- В этом плане мы пишем **rendering engine с нуля**, а JS-движок **встраиваем готовый** (см. §6.8).

---

## 1. Принципы проекта

1. **Приватность по умолчанию.** Никакой телеметрии, никаких аккаунтов, никаких облачных сервисов без явного включения.
2. **Лёгкость.** Цель — холодный старт < 300 мс, ~100 МБ RAM на пустой вкладке.
3. **Контролируемая поверхность.** Поддерживаем только нужные веб-API. Экзотика (WebUSB, WebBluetooth, WebMIDI, WebSerial, FedCM, Payment Request, WebNFC) — не реализуется.
4. **Прозрачность.** Каждый исходящий байт виден пользователю.
5. **Стабильный UI.** Минимализм, без «редизайнов» каждый релиз.
6. **Memory safety.** `unsafe` только на FFI-границах, всё ревьюится.
7. **Русский язык — first-class.** Кодировки, шрифты, IDN, локаль, переводы — на всех этапах, а не отложенная «фаза i18n». Подробности в §10.
8. **Knowledge layer как ценность для пользователя.** Браузер хранит и индексирует то, что пользователь видел / отметил / сохранил, локально. Это закрывает запросы, которые мейнстрим-браузеры не закрывают по бизнес-причинам: полнотекстовый поиск по истории, аннотации, офлайн-чтение, опциональный локальный AI-ассистент. Подробности в §12.

---

## 2. Реалистичный scope движка

Полный веб-стандарт нереалистичен. Мы целимся в **подмножество**, постепенно расширяя.

### v0.1 — «текстовый веб» (читалка)
- HTML5 (без `<form>` пока)
- CSS 2.1 + box model + блочный/инлайн layout
- Картинки (PNG, JPEG)
- HTTP/1.1, HTTPS
- Без JS

Цель: открывать энциклопедийные статьи, MDN, GitHub README, статьи блогов.

### v0.5 — «интерактивный читатель»
- CSS Flexbox
- Формы, базовый ввод
- JS через embedded QuickJS (без сложных Web APIs)
- HTTP/2
- WebFonts (WOFF2)

Цель: открывать форумы, Hacker News, простые SPA.

### v1.0 — «современный браузер»
- CSS Grid, transforms, animations
- Canvas 2D
- Полноценный JS через V8/SpiderMonkey
- DOM API (полное подмножество HTML Living Standard)
- Fetch, XHR, WebSocket
- IndexedDB, localStorage
- HTTP/3
- Service Workers (опционально)

Цель: открывать большинство сайтов, кроме самых тяжёлых SPA.

### Что НЕ берём (и в v1.0 тоже)
- WebGL, WebGPU (отдельный масштабный проект)
- WebRTC (медиа-кодеки = огромный отдельный мир)
- DRM / Widevine
- WebAssembly (можно добавить, когда JS-движок встанет)
- WebUSB / WebBluetooth / WebMIDI / WebSerial / WebNFC / Payment Request
- Native messaging для расширений
- PDF viewer (отдельным приложением или библиотекой)

---

## 3. Архитектура высокого уровня

```
┌──────────────────────────────────────────────────────────┐
│                    UI Process (shell)                    │
│   winit ▸ wgpu ▸ egui ▸ tabs ▸ omnibox ▸ shortcuts       │
└──────────────────┬───────────────────────────────────────┘
                   │ typed IPC (postcard over pipes)
       ┌───────────┼────────────┬──────────────┐
       ▼           ▼            ▼              ▼
   ┌────────┐  ┌────────┐  ┌────────┐   ┌────────────────┐
   │Renderer│  │Renderer│  │Renderer│   │ Network Service│
   │ proc 1 │  │ proc 2 │  │ proc N │   │  (one process) │
   │        │  │        │  │        │   │                │
   │ engine │  │ engine │  │ engine │   │ HTTP/TLS/DNS   │
   │ + JS   │  │ + JS   │  │ + JS   │   │ Filters/Cache  │
   └────────┘  └────────┘  └────────┘   └────────────────┘
                                                 │
                                        ┌────────┴────────┐
                                        │ Storage Service │
                                        │ cookies, idb,   │
                                        │ history (SQLite)│
                                        └─────────────────┘
```

- **UI process** — единственный, кто рисует окно и принимает ввод.
- **Renderer process на каждый origin** — site isolation как в Chromium. Краш одной вкладки не валит браузер. Эксплойт в одной вкладке не лезет в другую.
- **Network service** — единственный, кто ходит в сеть. Все TLS, DNS, фильтры рекламы, кэш — здесь. Центральная точка приватности.
- **Storage service** — единственный, кто пишет на диск (кроме логов и кэша). Cookies, IndexedDB, история, закладки.

IPC через `postcard` (компактный, бинарный, serde-совместимый) поверх:
- Unix: `tokio::net::UnixStream`
- Windows: Named Pipes
- macOS: Unix Domain Sockets

---

## 4. Структура репозитория

```
lumen/
├── Cargo.toml                     # workspace
├── crates/
│   ├── shell/                     # UI process
│   ├── ipc/                       # типы сообщений, транспорт
│   │
│   ├── engine/                    # сам движок
│   │   ├── html-parser/           # токенизатор + tree construction
│   │   ├── css-parser/            # токенизатор + grammar
│   │   ├── dom/                   # DOM-дерево, события
│   │   ├── style/                 # каскад, computed values
│   │   ├── selectors/             # матчинг CSS-селекторов
│   │   ├── layout/                # box generation, layout algorithms
│   │   ├── paint/                 # display list, рисование
│   │   ├── compositor/            # слои, GPU-композитинг
│   │   ├── text/                  # shaping, bidi, line breaking
│   │   ├── image/                 # декодирование PNG/JPEG/WebP
│   │   ├── font/                  # загрузка шрифтов, WOFF2
│   │   └── js-binding/            # мост к JS-движку
│   │
│   ├── webapi/                    # реализация Web API
│   │   ├── dom-api/               # document.querySelector и т.д.
│   │   ├── fetch/                 # fetch(), XHR
│   │   ├── canvas/                # Canvas 2D
│   │   ├── storage/               # localStorage, sessionStorage
│   │   └── timers/                # setTimeout, requestAnimationFrame
│   │
│   ├── renderer/                  # renderer process: связывает engine + webapi
│   │
│   ├── network/                   # network service
│   │   ├── http/                  # HTTP/1.1, /2, /3
│   │   ├── tls/                   # rustls wrapper
│   │   ├── dns/                   # DoH, DoT, обычный
│   │   ├── cache/                 # HTTP cache
│   │   ├── cookies/               # cookie jar с партиционированием
│   │   └── filters/               # свой adblock-матчер
│   │
│   ├── storage/                   # storage service (SQLite + in-memory)
│   │
│   ├── profiles/                  # управление профилями, шифрование
│   │
│   └── common/                    # общие типы, конфиг, URL parsing
│
├── assets/                        # иконки, default filter lists
├── tests/
│   ├── wpt/                       # Web Platform Tests subset
│   └── snapshots/                 # render snapshot tests
├── docs/
└── xtask/                         # build, release tasks
```

---

## 5. Технологический стек

### Политика зависимостей

**Стратегия (обновлено 2026-05-15): сначала рабочий браузер, потом разговор «что переписывать самим».** Раньше §5 формулировался бинарно — «всё своё, кроме 5 exception». На практике это упирало в задачи, которые ничего не определяют в идентичности Lumen, но стоят месяцев работы (image decoders, Unicode UAX-таблицы, Brotli, WOFF2, HTTP/3). Новая формулировка — две категории exception:

- **Permanent.** Никогда не пишем сами. Универсальное правило безопасности / здравого смысла. 5 шт.
- **Provisional accelerators.** Берём готовое сейчас ради скорости Phase 1-3, но за trait-anchor в `lumen-core::ext`, чтобы при желании заменить. У каждого — «graduation criterion»: событие, при котором имеет смысл писать своё. Большинство criterion-ов в духе «реалистично — никогда» (формат стабильный, без архитектурной ценности для Lumen) — это не лицемерие, а честная маркировка.

**«Не делаем Google Chrome» — это про ядро.** Lumen остаётся проектом про собственный rendering engine. HTML/CSS/DOM/style/layout/paint/font/encoding, URL, HTTP/1.1, DNS-резолвер с DoH/DoT, adblock matcher, knowledge layer (§12), UI shell — всегда наши. Если кто-то предлагает «возьми готовое» для пункта из этого списка — это уже Chrome-форк, а не Lumen.

Поэтому мы по-прежнему пишем **свой** код для:

- HTML / CSS парсеров, DOM, style cascade, selectors;
- layout (block, inline, flex, grid), paint, compositing;
- URL-парсинга и базового Punycode (RFC 3492 — IDNA UTS#46 опционально через provisional `idna`);
- HTTP/1.1, HTTP/2, DNS-резолвера с DoH/DoT (HTTP/3 — через provisional `quinn`);
- определения и конвертации кодировок (cp1251, KOI8-R, CP866 и др.);
- PNG-декодера (готов в `lumen-image` + свой DEFLATE, переиспользуемый для HTTP gzip/deflate); JPEG/WebP/GIF — через provisional image-decoder-crate;
- TrueType-парсинга и text shaping для Latin / Cyrillic (WOFF2 — через provisional `woff2`);
- движка адблок-фильтров;
- 2D-растеризации поверх GPU-абстракции;
- ephemeral KV-хранилища (in-memory, для тестов и session-scope данных);
- IPC, async-примитивов, work-stealing thread pool;
- UI-фреймворка (иммедиат-режим поверх своих paint-примитивов);
- собственных MD5 / SHA-256 (для HTTP Digest, не security-критично — challenge-response, не KDF), Base64;
- knowledge layer §12 — это пользовательская ценность Lumen, не делегируется внешним библиотекам.

Bidi (UAX #9), line breaking (UAX #14), segmentation (UAX #29), normalization (UAX #15) — формально были в «своё», но писать свои Unicode-таблицы — это годы работы с обновлениями при каждом релизе Unicode. Переходят в provisional через `icu4x`.

### Permanent exceptions (5 шт., никогда не переписываем)

Это единственные deps, для которых принципиально нет смысла писать своё. Каждая прячется за trait в [`lumen-core::ext`](crates/core/src/ext.rs).

| Crate | Что покрывает | Trait-anchor | Почему не сами |
|---|---|---|---|
| **`winit`** | OS event loop, окна, ввод | `WindowingBackend` | Win32 + X11 + Wayland + AppKit — ~50–100k LOC платформенно-специфичных багов и behaviour quirks |
| **`wgpu`** | GPU API (Vulkan / Metal / DX12 / GL) | `RenderBackend` | 4 разных API, разные семантики, driver-баги. Свой = годы работы и регрессий |
| **`rustls`** + **`webpki-roots`** | TLS, X.509, X25519, AES-GCM, HKDF; `webpki-roots` — bundle корневых CA-сертификатов (Mozilla CA bundle). Без него HTTPS не валидируется. | `TlsBackend` | **Универсальное правило безопасности:** не пишите свой crypto. rustls — аудит + формальная верификация частей кода. `webpki-roots` — pure data + lookup, partner-crate к rustls |
| **SQLite** (`rusqlite` с `bundled` feature) | Персистентное хранилище: history, bookmarks, notes, read-later, cookies-TTL, профили. FTS5 для §12.1 полнотекстового поиска. | `StorageBackend` + `KnowledgeStore` | 25 лет TH3-тестирования (100% MC/DC branch coverage), стандарт индустрии браузеров (Firefox/Chromium/Safari). Цена ошибки persistent storage асимметрична — молчаливая порча данных пользователя; та же логика, что у crypto. FTS5 закрывает §12.1 без своего inverted index |

> **Долгосрочная стратегия: pure-Rust storage (redb + tantivy).** SQLite остаётся permanent exception на Phase 0–2. Однако `rusqlite` с `bundled` тянет ~250 КБ C-кода, который нельзя аудировать средствами Rust — это противоречит принципу «свой код = прозрачность». Целевая архитектура (Phase 3+): **redb** (pure Rust, ACID copy-on-write B+tree, ноль `unsafe`, используется в `cargo`) для key-value подсистем (localStorage, sessionStorage, IndexedDB, HTTP cache) + **tantivy** (Rust-native FTS) для полнотекстового поиска §12.1. Оба за `StorageBackend` / `KnowledgeStore` trait — drop-in замена. Graduation criterion: замерить p99 latency SQLite WAL vs redb на реальной нагрузке; если SQLite < 1 мс — миграция не срочна, но остаётся целью ради чистоты стека.
| **JS engine** (`rquickjs` v0.5 → `rusty_v8` v1.0+) | Исполнение JavaScript | `JsRuntime` | V8 — 15 лет, миллиарды долларов, сотни инженеров. QuickJS на старте, V8 в v1.0+ |

### Provisional accelerators (берём готовое сейчас, заменяем по событию)

Trait-anchor у каждого — в `lumen-core::ext`. Подключаем по мере того, как фаза реально упирается в задачу. Список открыт.

| Crate (кандидаты) | За что | Trait-anchor | Phase | Graduation criterion |
|---|---|---|---|---|
| `zune-jpeg`, `image-webp`, узкий `image` без default features | Декодирование JPEG / WebP / GIF в RGBA. PNG **остаётся свой** в `lumen-image` | `ImageDecoder` | 1 | Едва ли когда-то. Форматы стабильные, без архитектурной ценности; цена реализации (JPEG — DCT+Хаффман+chroma subsampling+progressive; WebP — VP8/VP8L) непропорциональна выгоде |
| `icu4x` (выборочные модули: segmentation, line-break, bidi, normalization, CLDR-минимум) | Unicode UAX #9 / #14 / #29 / #15 + локалевые таблицы | `UnicodeProvider` | 1–2 | Реалистично — никогда. Unicode Consortium = «универсальное правило безопасности» для Unicode, аналогично rustls для crypto. Своя реализация = годы поддержки таблиц на каждом релизе Unicode |
| `brotli-decompressor` | Brotli decompression для HTTP `Content-Encoding: br` | расширение `ContentDecoder` | 1–2 | Едва ли. Формат RFC 7932 стабилен, своя реализация = недели с собственным dictionary |
| `ruzstd` / `zstd-safe` | Zstandard decompression для HTTP `Content-Encoding: zstd` (Cloudflare и nginx уже отдают; через 1-2 года будет распространено) | расширение `ContentDecoder` | 1–2 | Реалистично — никогда. Формат RFC 8478 стабилен, без архитектурной ценности; своя реализация = недели |
| `publicsuffix` (или собственный загрузчик `publicsuffix.org/list/public_suffix_list.dat`) | Public Suffix List для cookie domain matching (`example.co.uk` ≠ `co.uk`), eTLD+1 расчёта, `SameSite=Strict` boundary | `PublicSuffixList` | 1 | Едва ли. Данные обновляются раз в неделю-месяц, формат — простой текст; собственный loader тривиален, но crate избавляет от поддержки парсера |
| `idna` | Полный UTS#46 mapping table для IDN (ß, ZWJ, контекстные правила) | `IdnaProvider` (на базе текущего `Url::host_ascii()`) | 1–2 | Когда найдём real edge-case, который наш `str::to_lowercase`-Punycode не покрывает |
| `hyphenation` | Перенос слов (TeX-словари, включая русский) | `HyphenationProvider` | 2 | Phase 2+ при типографике. Словари можно переписать на свой формат, но low priority |
| `woff2` | Распаковка WOFF2 в TTF | расширение `FontFormat` | 2 | Phase 2 при WebFonts. Формат стабилен, маловероятно писать своё |
| `hunspell-rs` / `spellbook` | Spell-check (русская морфология обязательна) | `SpellChecker` | 3 | Phase 3 при spell-check. Морфология русского сложна, цена своей реализации перекрывает выгоду |
| `quinn` | HTTP/3 / QUIC | расширение `NetworkTransport` | 3 | Реалистично — никогда. QUIC = год+ работы (congestion control, packet loss recovery, 0-RTT, key updates) |
| `redb` | Pure Rust ACID key-value (copy-on-write B+tree). Альтернативный storage backend для горячих key-value (localStorage, IndexedDB, HTTP cache) | `StorageBackend` | 2–3 | Замерить p99 latency SQLite WAL vs redb на реальной нагрузке localStorage/IndexedDB. Если SQLite < 1 мс — не нужен |
| `tantivy` | Rust-native полнотекстовый поиск. Замена SQLite FTS5 для §12.1 knowledge layer при миграции на pure-Rust storage | `KnowledgeStore` | 3+ | Только вместе с redb — при решении полностью отказаться от SQLite C-кода |

**Принципы работы с provisional-категорией:**

- **Trait-anchor обязателен.** Перед добавлением dep в `Cargo.toml` сначала появляется trait в `lumen-core::ext` и default-имплементация (наша, заглушечная или wrapped-around готового crate). Это гарантирует, что замена в будущем — drop-in, без переписывания потребителей.
- **Подключение «по событию», не превентивно.** Не добавляем `icu4x` пока bidi реально не понадобится; не добавляем `quinn` пока HTTP/3 не на повестке.
- **Annual review.** Раз в год — проход по provisional-списку: какие graduation criteria сработали → завести задачу на свой код; какие нет → продлить.
- **Расширение списка — через DECISIONS.md.** Каждое добавление в provisional — новая запись в [DECISIONS.md](DECISIONS.md) с обоснованием и graduation criterion.

### Что НЕ берём как зависимости (даже временно — ядро Lumen)

Эти крейты регулярно обсуждаются как «возьми готовое», но для всех решение — **отвергнуть**. Это идентичность проекта.

- ~~`html5ever`~~ → свой HTML-парсер по [HTML5 spec](https://html.spec.whatwg.org/multipage/parsing.html) (см. §6.1).
- ~~`cssparser` + `selectors`~~ → свой CSS-парсер по CSS Syntax L3 (§6.2).
- ~~`stylo`~~ → свой каскад и computed values (§6.4).
- ~~`taffy`~~ → свой layout: block, inline, flex, grid (§6.5).
- ~~`tiny-skia`~~ → свой 2D-растеризатор (CPU для v0.1, GPU через `wgpu` дальше).
- ~~`hyper`~~ → свой HTTP/1.1 и HTTP/2 поверх `rustls` + std (только HTTP/3 через provisional `quinn`).
- ~~`hickory-resolver`~~ → свой DNS-резолвер с DoH/DoT поверх `rustls`.
- ~~`ttf-parser` / `font-kit`~~ → свой TrueType-парсер и font matcher (только WOFF2-распаковка через provisional `woff2`).
- ~~`rustybuzz`~~ → свой shaper для Latin / Cyrillic. Сложные скрипты (арабский, индийский, тайский) — в v1.0+, отдельным модулем; пока «не поддерживается».
- ~~`encoding_rs`~~ → свои таблицы декодирования (cp1251, KOI8-R, CP866, UTF-8, ASCII, Win-1252).
- ~~`url`~~ → свой URL parser по WHATWG URL spec (текущий стаб в `lumen-core::url`).
- ~~`unicode-security`~~ → свои homograph checks для IDN.
- ~~`adblock`~~ (Brave) → свой filter matcher.
- ~~`readability`~~ → своя реализация readability heuristics с настройкой под кириллицу (§10.9).
- ~~`postcard` + `serde`~~ → своя компактная binary serialization для IPC.
- ~~`tokio`~~ → свой минимальный async-исполнитель поверх std + epoll/kqueue/IOCP (или single-threaded на старте).
- ~~`rayon`~~ → свой work-stealing thread pool, когда понадобится параллельный layout / style.
- ~~`egui` / `iced` / `Slint`~~ → свой иммедиат-режим UI поверх `wgpu`-примитивов из paint-крейта.
- ~~`flate2` / `miniz_oxide`~~ для PNG — отвергнуто (см. [DECISIONS.md](DECISIONS.md)). PNG-декодер с собственным DEFLATE уже написан в `lumen-image`; DEFLATE переиспользуется для HTTP `Content-Encoding: gzip/deflate`.

### Devtools (не runtime — допустимы)

Инструменты, которые не попадают в бинарь, но используются на CI / при разработке:

- `cargo-deny` — аудит лицензий и CVE четырёх exceptions и их транзитивных зависимостей.
- `cargo-vet` — supply-chain reviews.
- `cargo-dist` — упаковка релизов (опционально).
- `cross` — кросс-компиляция на CI.

### Принцип «no new dep без обоснования»

Если в коммите / Pull Request добавляется новая зависимость в `Cargo.toml`, в описании обязателен пункт:

> **Why this dependency:** \<обоснование, почему свой код тут категорически неуместен — иначе пишем сами\>

CI-чек на новые `[dependencies]`-строки добавим, когда появится remote.

### Язык и тулинг

- **Rust** edition 2024, MSRV — последний stable (сейчас 1.95).
- `cargo` workspace.
- Сборка релизов — `xtask`-крейт, опционально `cargo-dist` поверх.

---

## 6. Движок: компоненты детально

### 6.1 HTML parser

**Что это:** превращает поток байт в DOM-дерево по спеке [HTML5 parsing algorithm](https://html.spec.whatwg.org/multipage/parsing.html).

**Состоит из:**
- **Tokenizer** — конечный автомат с ~80 состояниями. Принимает байты, выдаёт токены: `StartTag`, `EndTag`, `Character`, `Comment`, `Doctype`.
- **Tree construction** — берёт токены и строит DOM с учётом «insertion modes» (~20 режимов). Тут вся магия: `<table>` особо обрабатывает `<tr>`, `<form>` нельзя вложить в `<form>` и т.д.
- **Encoding sniffing** — определение кодировки из BOM, meta, заголовков.

**Crate (свой):** `engine/html-parser`. Пишем с нуля по HTML5 spec.

**Сложность:** не алгоритмическая, а в точности следования спеке. Тесты — `html5lib-tests` (10 тыс. testcases).

### 6.2 CSS parser

**Что это:** байты → CSSOM (StyleSheet → Rule → Declaration → Value).

**Состоит из:**
- **Tokenizer** по [CSS Syntax Level 3](https://www.w3.org/TR/css-syntax-3/).
- **Parser** для разных грамматик: selector, declaration, at-rule (`@media`, `@font-face`, `@keyframes`, `@supports`, `@container`).
- **Value parser** для каждого property (color, length, calc(), gradient, transform-function...).

**Свой парсер.** Пишем токенизатор + parser по CSS Syntax L3 spec; селекторы — по CSS Selectors L4. Не берём `cssparser`/`selectors` (см. политику §5).

**Сложность:** объём. CSS properties — 600+. Реализуем по приоритету (display, position, margin, padding, color, font, background — первая сотня покрывает 95% сайтов).

### 6.3 DOM

**Что это:** дерево узлов в памяти + API мутаций + события.

**Ключевые решения:**
- **Хранение:** не наивные `Rc<RefCell<Node>>` (слишком медленно, циклы), а **арена** (`Vec<NodeData>`) с `NodeId(u32)`. Так делает Servo. Дёшево клонировать, кэш-дружелюбно.
- **Сильные/слабые ссылки:** parent-child через индексы, никаких `Rc`-циклов.
- **Mutations:** все через mutator API, чтобы записывать инвалидацию стилей/layout.
- **Events:** capture/bubble фазы, ленивая регистрация listeners.
- **MutationObserver** — поддерживаем (нужен для современных фреймворков).

**Crate:** `engine/dom`.

### 6.4 Style system (cascade)

**Что это:** для каждого DOM-узла вычислить **computed style** — финальные значения всех CSS-property.

**Этапы:**
1. **Selector matching:** для каждого узла найти все matching rules. Оптимизация — bloom filter ancestor cache (как в WebKit/Blink).
2. **Cascade:** отсортировать по специфичности + origin (user-agent / user / author) + `!important`.
3. **Inheritance:** свойства типа `color`, `font-*` наследуются.
4. **Computed values:** `em` → `px`, `red` → rgba, относительные → абсолютные.

**Параллельность:** style resolution параллелится по поддеревьям через `rayon`. Это главное преимущество Servo-подхода.

**Своя реализация.** Bloom-filter ancestor cache, параллельный matching через свой work-stealing pool. Не берём `stylo` (см. §5).

**Crate:** `engine/style`.

### 6.5 Layout

**Что это:** computed style + DOM → дерево боксов с координатами и размерами.

**Алгоритмы по приоритету:**
1. **Block & inline (CSS 2.1)** — базис. Block formatting context, inline formatting context, line boxes.
2. **Floats & clear** — устаревшее, но ещё много где встречается.
3. **Positioning** — static / relative / absolute / fixed / sticky.
4. **Flexbox** — `flex-direction`, `justify-content`, `align-items`, `flex-grow/shrink/basis`.
5. **Grid** — самый сложный. Track sizing algorithm, named lines, auto-placement.
6. **Tables** — отдельный мир алгоритмов (table-fixed vs table-auto layout).
7. **Multi-column, transforms, writing-modes** — позже.

**Архитектура:** layout tree отдельно от DOM (как в Blink/Servo). Один DOM-узел может породить несколько layout-боксов (анонимные боксы, `::before`/`::after`).

**Своя реализация.** Block + inline на старте (Phase 0), затем flex (Phase 2), grid (Phase 3). Не берём `taffy` (см. §5) — алгоритмы Grid и Flex описаны в spec, реализуемы.

**Crate:** `engine/layout`.

### 6.6 Paint

**Что это:** layout tree → display list (список команд рисования: «нарисовать прямоугольник 10,10–100,50 цвета red»).

**Команды display list:**
- `DrawRect(rect, paint)`
- `DrawText(glyphs, position, font, paint)`
- `DrawImage(image, src_rect, dst_rect)`
- `DrawPath(path, paint)` (для borders, gradients)
- `PushClip(rect)` / `PopClip`
- `PushTransform(matrix)` / `PopTransform`
- `PushOpacity(alpha)` / `PopOpacity`

**Почему display list:** разделяет «что рисовать» от «как рисовать». Удобно для:
- кэширования (если layout не поменялся — переиспользуем),
- передачи в compositor,
- тестирования (snapshot-тесты на display list, а не на пиксели).

**Crate:** `engine/paint`.

### 6.7 Compositor

**Что это:** превращает display list в реальные пиксели через GPU.

**Подход:** **WebRender-style** — каждый кадр выгружается в GPU как набор примитивов, GPU параллельно растеризует. Никаких CPU-растеризованных слоёв.

- Слои для `position: fixed`, `transform`, `opacity`, `will-change`.
- Tiling для больших страниц (рисуем только видимое + буфер).
- Анимации через compositor (transform/opacity без relayout).

**Стек:** `wgpu` (под Vulkan/Metal/DX12/GL). Свои шейдеры на WGSL.

**Своя реализация.** На старте — простой CPU-растеризатор (line/rect/path/text) в `lumen-paint`. С v0.5 — GPU-pipeline поверх `wgpu` (единственная внешняя зависимость в этом слое, см. §5). Не берём `tiny-skia` / `skia`.

**Crate:** `engine/compositor`.

### 6.8 JS engine integration

**Решение:**
- **v0.1:** без JS.
- **v0.5:** **QuickJS** через `rquickjs` crate. Маленький (~200 КБ), ES2020-совместимый, простой биндинг. Медленнее V8 в 10–50 раз, но для не-SPA сайтов хватает.
- **v1.0:** **V8** через `rusty_v8` (Deno-style) или **SpiderMonkey** через `mozjs`. V8 быстрее, SpiderMonkey ближе по духу. **Рекомендация: V8** — больше документации, тесты Deno как референс.

**Биндинги (важно):**
- Каждый Web API экспортируется в JS как объект/функция.
- Биндинги генерируем из WebIDL (`weedle` crate для парсинга IDL).
- Сборщик мусора JS-движка должен «видеть» Rust-объекты, к которым держит ссылки. У V8 — wrapper objects + tracing handles. Это **самая хрупкая граница** проекта.
- `unsafe` неизбежен на этой границе. Изолируем в `engine/js-binding`, ревью + fuzzing обязательны.

**Crate:** `engine/js-binding` + `webapi/*`.

### 6.9 Text rendering

**Этапы:**
1. **Font matching** — найти шрифт для каждого глифа (CSS font fallback chain).
2. **Shaping** — текст + шрифт → последовательность глифов с позициями. `rustybuzz`.
3. **Line breaking** — `xi-unicode` (Unicode UAX #14).
4. **Bidi** — `unicode-bidi` (UAX #9). Арабский, иврит.
5. **Rasterization** — `ab_glyph` или `fontdue` для CPU, или прямо на GPU через signed distance fields.

**Crate:** `engine/text`.

### 6.10 Image decoding

`image` crate покрывает PNG, JPEG, GIF, WebP, BMP, ICO. AVIF — через `libavif` (C dep). SVG — через `resvg` (Rust). Все декодируем **в renderer-процессе**, не в network. Это важно для безопасности: декодеры — частый источник CVE.

**Crate:** `engine/image`.

---

### 6.11 Automation API (lumen-driver)

Полное обоснование архитектурных решений — [ADR-006](docs/decisions/ADR-006-automation-api.md).

Automation — **first-class поверхность движка**, не пристройка debug-протокола. Один внутренний trait `BrowserSession` в крейте `lumen-driver`, три транспорта поверх него (in-process Rust / MCP / WebDriver BiDi). Это даёт три эффекта одновременно:

1. **Собственное тестирование изнутри** — `graphic_tests/` мигрируют с пиксельных diff-ов против Edge на структурные числовые ассерты (`box.border_box.width == 200.0`) плюс in-process snapshot tests без ffmpeg / gdigrab / Windows-only crop offsets. Запускается за миллисекунды, кросс-платформенно, на любом CI.
2. **Эмбеддинг как библиотека** — `cargo add lumen-driver` даёт чужому Rust-приложению полноценный браузерный движок с API «открой → проверь layout → кликни», без отдельного процесса.
3. **Внешние клиенты автоматизации** — AI-агенты через MCP, Playwright/Selenium/Cypress через BiDi, без специальных обёрток.

#### `BrowserSession` trait — поверхность

| Группа | Методы |
|---|---|
| **Lifecycle** | `new(opts)`, `navigate(url)`, `reload()`, `close()` |
| **Computer-use primitives** (vision-агенты) | `screenshot(opts)`, `input_event(ev)` — нативная инжекция, **не** synthetic JS, `viewport(w, h)` |
| **Semantic surface** | `a11y_tree()`, `query(Query::Role/Name/Text/Css)`, `layout_box(handle)`, `computed_style(handle)`, `eval_js(code)` |
| **Wait conditions** (auto-wait внутри движка) | `wait_for(Cond::Visible/Stable/NetworkIdle/JsIdle)` |
| **Observability** | `network_log()`, `console()`, `display_list()` |
| **Determinism** | `set_clock(ClockMode)`, `set_rng_seed(u64)`, `freeze_fingerprint(profile)` |
| **Storage / context** | `cookies()`, `local_storage()`, изоляция per-session по умолчанию |

#### Транспорты (поверх trait-а)

| Крейт | Протокол | Кто потребляет |
|---|---|---|
| `lumen-driver` | Rust API in-process | Свои `graphic_tests/`, embed-пользователи |
| `lumen-mcp-server` | Model Context Protocol (JSON-RPC over stdio/socket) | Claude Computer Use, OpenAI Operator/CUA, Browser Use, локальные LLM-агенты |
| `lumen-bidi-server` | WebDriver BiDi (W3C, WebSocket) | Playwright, Selenium 5, Cypress |
| `lumen-cdp-shim` (опционально, **по запросу**) | Chrome DevTools Protocol subset | Legacy Puppeteer — только если будет реальный спрос |

#### Что Lumen даёт сверх BiDi-спеки

W3C BiDi на май 2026 — Working Draft с известными пробелами (см. blocker-issues Playwright #32577 и Cypress #30447). Lumen реализует их **с первого дня**, потому что контролирует свой стек:

- Полный доступ к response body и `resourceType` в network events
- Locale / timezone / offline emulation
- Per-context user-agent и extra HTTP headers
- Viewport до загрузки popup / new tab
- Preload-скрипты per browsing context
- Полный download lifecycle (begin → progress → complete + body)
- Cookie change events, per-origin storage clear
- Дешёвая network interception (не «prohibitively expensive» как в текущей BiDi)

#### Что Lumen **не** делает

- **WebDriver Classic** — мёртвый HTTP request-response протокол, в проект не входит.
- **CDP как primary** — Lightpanda пошёл этим путём и теперь несёт груз нестабильного API. У нас CDP может появиться **только как thin shim** в Phase 3+ при реальном спросе.
- **DOM-селекторы как primary локаторы** — поддерживаются как fallback, но рекомендуются role+name запросы по a11y-tree. Это снимает 70% maintenance-боли тестов (industry data 2026).
- **Synthetic JS events (dispatchEvent) для input** — анти-боты их распознают, реальные сайты с `event.isTrusted` ведут себя иначе. Только нативная инжекция через event loop шелла.
- **Wait-логика в клиенте** — auto-wait живёт в движке (на тик layout/network/JS-idle), не в SDK retry-loop.

**Crates:** `lumen-driver` (P3 owner), `lumen-mcp-server` (P3), `lumen-bidi-server` (P3). A11y tree строится в `lumen-a11y` (P1 owner) — automation использует её как готовую структуру, не дублирует.

---

## 7. Web APIs

Реализуем по приоритету.

### Tier 1 (нужны для большинства сайтов)
- `document.*`, `Element.*`, `Node.*` — DOM API
- `querySelector`, `querySelectorAll`
- `addEventListener`, `removeEventListener`
- `fetch()`, `XMLHttpRequest`
- `localStorage`, `sessionStorage`
- `setTimeout`, `setInterval`, `requestAnimationFrame`
- `console.*`
- `window.location`, `window.history`
- `URL`, `URLSearchParams`
- `FormData` ✅, `Blob` ✅, `File` ✅, `FileReader` ✅, `btoa`/`atob` ✅
- `Promise` (даёт JS-движок)

### Tier 2
- `Canvas 2D`
- `IndexedDB`
- `WebSocket`
- `MutationObserver`, `IntersectionObserver`, `ResizeObserver`
- `requestIdleCallback`
- Clipboard API (read/write с разрешения)

### Tier 3 (опционально)
- Service Workers
- Web Workers
- Shadow DOM
- Custom Elements
- WebAssembly (через JS-движок «бесплатно»)

### Не реализуем
- WebUSB, WebBluetooth, WebMIDI, WebSerial, WebNFC, Payment Request, FedCM, WebHID, EME (DRM), Background Sync, Push, Notifications API (на старте).

**Crate:** `webapi/*`.

---

## 8. UI оболочка

### 8.1 Технологический выбор
- **`winit`** — окна, события.
- **`wgpu`** — рендеринг UI и engine compositor через один GPU-контекст.
- **`egui`** для v0.1–v1.0 — иммедиат-режим, очень быстро разрабатывается, кросс-платформенный.
- Возможный переход на `iced` или `Slint` к 2.0 для более polished UX.

### 8.2 Структура UI
```
┌──────────────────────────────────────────────────┐
│ [≡] [◀][▶][↻] [omnibox.................][⋯][↓]  │  toolbar
├────┬─────────────────────────────────────────────┤
│ ▾ Work             ┌─────────────────────────┐   │
│  ├ GitHub          │                         │   │
│  ├ Linear          │      Active tab         │   │
│  └ Docs            │      content area       │   │
│ ▾ Personal         │                         │   │
│  ├ HN              │                         │   │
│  └ Mail            └─────────────────────────┘   │
│ + New tab                                        │
├────┴─────────────────────────────────────────────┤
│ Network log: 12 req, 340 KB, 4 blocked          │  status bar
└──────────────────────────────────────────────────┘
```

### 8.3 Возможности UI

**Базовые:**
- Адресная строка (omnibox) с локальным поиском по истории/закладкам. Поисковые подсказки — **выключены по умолчанию**.
- Вкладки: вертикальные с деревьями (parent → children).
- Закладки: дерево, теги.
- История: полнотекстовый поиск по локальной БД.
- Find in page (Ctrl+F).
- Zoom (Ctrl+/Ctrl-).

**Продвинутые:**
- **Workspaces** — наборы вкладок, переключение Ctrl+1..9. Каждый — со своим контекстом cookies (опционально).
- **Tab tree** — вкладки иерархично, складываются по группам.
- **Tab hibernation** — фоновые вкладки выгружаются через N минут. Вкладка **остаётся видимой** в списке, активируется по клику.
- **Tab auto-archive** — отдельная семантика от hibernation: после N часов неактивности вкладка убирается из видимого списка во **внутренний archive**, остающийся доступным через `@history` / `@tabs`-поиск (§12.4) и FTS-индекс (§12.1). Запинованные вкладки не трогаются. Аналог `Today`-секции в Arc; решает «tab hoarding» без потери содержимого. Конкретный таймаут N — настройка пользователя, по умолчанию 12 ч.
- **Split view** — две вкладки рядом.
- **Picture-in-picture** для видео.
- **Reader mode** (Ctrl+R) на основе `readability`.
- **Команд-палитра** Ctrl+Shift+P — все действия клавиатурой (как VS Code).
- **Network log панель** — что уходит, куда, сколько байт, что заблокировано.

**Темы:**
- Light, dark, system, AMOLED-black.
- Без анимаций по умолчанию (можно включить).
- Без округлых иконок 12-цветной палитры — функциональный минимум.

### 8.4 Чего НЕ делаем в UI
- Лент новостей, рекомендаций, шопинга, погоды.
- ИИ-сайдбара по умолчанию.
- Welcome-screens, туториалов, бейджей.
- «Вы давно не заходили» нотификаций.
- Forced sign-in.

---

## 9. Приватность

### 9.1 Сетевой уровень

**DNS:**
- DoH (DNS over HTTPS) по умолчанию. Провайдеры — на выбор: Cloudflare 1.1.1.1, Quad9, NextDNS, свой.
- DoT (DNS over TLS) — альтернатива.
- DNS cache — в network service, не зависит от ОС.
- DNS-prefetch — выключен по умолчанию.

**TLS:**
- `rustls` only, никакого OpenSSL.
- Минимум TLS 1.2, по умолчанию 1.3.
- ECH (Encrypted Client Hello) — поддерживаем, когда доступно.
- TLS ClientHello fingerprint — нормализованный (uTLS-style), чтобы не выделяться.

**HTTP:**
- `Referer` на cross-origin — `strict-origin-when-cross-origin` по умолчанию.
- `User-Agent` — фиксированная строка (как у Tor Browser), без минорных версий ОС.
- `Accept-Language` — нормализованная.
- Strip URL params: `utm_*`, `fbclid`, `gclid`, `mc_*`, `_ga`, `yclid`, `igshid` и т.д. Списки обновляемые.

**Прокси:**
- SOCKS5, HTTP, HTTPS.
- Tor — нативная поддержка (запуск `tor` бинаря, либо `arti` — Rust Tor).
- Per-tab proxy — можно назначить разный прокси разным вкладкам.

### 9.2 Cookies и storage

- **Total cookie protection** — cookies партиционированы по top-level eTLD+1. Третьесторонний сайт получает свой cookie jar для каждого встраивающего сайта.
- **SameSite=Lax по умолчанию** — даже если сайт не указал.
- **First-Party Isolation** — IndexedDB, localStorage, cache — всё партиционировано.
- **Целевой pure-Rust backend (Phase 3+):** redb для горячих key-value (localStorage, sessionStorage, IndexedDB, HTTP cache) + tantivy для FTS — замена SQLite C-кода за `StorageBackend` trait (см. §5).
- **Auto-clear:** опционально, при закрытии вкладки/окна/сессии.
- **Cookie viewer** — UI для просмотра и удаления.

### 9.3 Профили

- Несколько изолированных профилей (личный/работа/анонимный/гость).
- Каждый — отдельная директория + отдельный мастер-ключ (Argon2id KDF из пароля).
- Storage внутри профиля шифруется (XChaCha20-Poly1305) — даже если кто-то получит диск.
- **Quick profile switch** — Ctrl+Shift+M.

### 9.4 Контентная фильтрация

- **Встроенный adblock — свой матчер.** Поддерживаем формат фильтров uBlock / EasyList (синтаксис задокументирован). Реализуем как `lumen-network::filters`. Не берём `adblock-rust` (см. §5).
- Подписки: EasyList, EasyPrivacy, uBO filters, NoCoin, Fanboy social.
- **Фильтрация на уровне network service** — НЕ зависит от движка. Сайт не может обойти через какой-нибудь Manifest V3-аналог.
- Cosmetic filtering (скрытие элементов) — через стили, инжектится в renderer.
- Per-site disable — пользовательский whitelist.

### 9.5 Anti-fingerprinting / Anti-detection privacy stack

Полное архитектурное обоснование и red lines — [ADR-007](docs/decisions/ADR-007-anti-detection-stack.md).

**Принцип:** пользователь имеет право посещать публичный сайт со своего устройства. Lumen — user agent в интересах пользователя, не сайта-оператора. Privacy-stack устанавливается **по умолчанию для всех** (как в Firefox Strict / Brave / Tor), не как opt-in «stealth mode». Побочный эффект — устойчивость к anti-bot системам (Cloudflare/DataDome/Akamai/PerimeterX/Kasada/Imperva), которые иначе ложно-помечают любой не-Chrome браузер.

Anti-detection покрывает **шесть слоёв**, потому что современные детекторы 2026 работают глубже, чем «canvas pixel hash»:

#### Слой 1 — Surface API: нет automation-маркеров (always-on, default)

- `navigator.webdriver` **не существует** (не `false`, а отсутствует — как в clean Chrome без `--enable-automation`).
- Нет `chrome.runtime`, `__playwright`, `__puppeteer`, `cdc_*` (ChromeDriver), `_phantom`, `callPhantom`, `Buffer`, `emit`-on-window и других классических маркеров.
- JS-runtime (`rquickjs` Phase 0, V8 Phase 3+) **не инструментирован** для automation. Автоматизация идёт через `BrowserSession` (см. §6.11, ADR-006) — она не касается JS-окружения, если страница сама к нему не обращается.
- `event.isTrusted = true` для native-injected input — события приходят в event loop тем же путём, что от ОС.

#### Слой 2 — TLS fingerprint (default + per-profile)

- `rustls` сконфигурирован с **cipher suite ordering, extension list и supported groups, совпадающими с current stable Chrome** (default profile). ALPN: `h2`, `http/1.1` — порядок Chrome-овский.
- Цель: сайт не должен мочь выделить юзера Lumen только потому, что мы выбрали другую Rust-TLS библиотеку, — мы конфигурируем `rustls` так, как `rustls` уже умеет конфигурироваться.
- **Per-profile**: privacy-strict профиль использует `rustls`-defaults, корпоративный — pinned JA3, Tor-profile — JA3 Tor Browser.
- **Что мы не делаем:** не патчим криптографию, не имитируем «быть» Chrome поверх собственной идентичности (UA остаётся `Lumen/0.x`).

#### Слой 3 — HTTP layer (default + per-profile)

- **HTTP/1.1**: порядок и casing заголовков (`User-Agent`, `Accept`, `Accept-Encoding`, `Accept-Language`, ...) — как у текущего Chrome.
- **HTTP/2**: `SETTINGS` frame values (`SETTINGS_HEADER_TABLE_SIZE = 65536`, `SETTINGS_MAX_CONCURRENT_STREAMS = 1000`, `SETTINGS_INITIAL_WINDOW_SIZE = 6291456`, …), stream priority frames — как у Chrome.
- `Accept-Language` по умолчанию `en-US,en;q=0.9` (не палит реальную локаль юзера); пользователь может переопределить вручную.
- Client Hints (`Sec-CH-UA`, etc.) — отдаём свой UA на запрос, либо ничего на Strict (как Tor).

#### Слой 4 — Rendering fingerprint (Brave-style, default)

Старый §9.5 — оставлен и формализован:

- **Canvas randomization** — `Canvas.getImageData` с микро-шумом, per-session deterministic seed (как Brave).
- **WebGL renderer / vendor** — обобщённые строки («Generic GPU», «WebKit»); shader compilation timing нормализован.
- **AudioContext fingerprint** — мизерный шум.
- **Fonts enumeration** — белый список + только bundled fonts на Strict.
- **Timezone** — опция UTC на Strict; иначе реальный.
- **Screen resolution** — округление до 100px на Strict.
- **Hardware concurrency** — фиксированное значение на Strict.
- **Battery API** — отключён (no information) на Strict.
- **WebRTC** — только mDNS host candidates, без public IP leak (как Brave/Safari).

#### Слой 5 — Behavioral input (opt-in **только для automation API**)

- `BrowserSession::input_event()` (см. §6.11 / ADR-006 task 8C) принимает `InputMode::HumanLike` опционально — Bézier-кривые движения мыши, variable inter-keystroke timing, малые dwell-time перед кликами.
- **Назначение** — тестировщики, которые хотят чтобы автотесты проходили те же code paths, что реальный юзер (event coalescing, hover transitions, slow-pointer logic). Это **не stealth-фича**, реальный человеческий input через шелл — уже человеческий и mimicry не требует.

#### Слой 6 — Профили (расширение существующих трёх)

- **Standard** (default) — Слои 1+2+3 + слой 4 на низкой интенсивности + total cookie protection + adblock + strip URL params. Сайты работают.
- **Strict** — Слои 1+2+3 + слой 4 на высокой интенсивности + WebRTC mDNS-only + Client Hints отключены + JS-блокировка на сомнительных доменах.
- **Tor-mode** — Strict + Tor circuit + Tor Browser JA3/UA/screen/font pinning + zero persistent state.
- **Per-context override** — `BrowserSession::set_fingerprint_profile(profile)` для automation-юзеров с конкретной identity (ADR-006 task 8F.3 уже включает `freeze_fingerprint`).

#### Red lines (никогда не делаем — см. ADR-007 «Consequences»)

- ❌ **CAPTCHA-solver** (on-device или через сервис) — у сайтов есть legitimate interest в human-verification для определённых функций.
- ❌ **Built-in IP rotation / residential proxy integration** — network identity это выбор и ответственность юзера, не функция движка.
- ❌ **Anti-fraud-detection bypass для банков, платежей, госуслуг** — эти системы защищают от реального вреда.
- ❌ **Marketing как «scraping browser» / «stealth automation»** — Lumen позиционируется как privacy-браузер; то что он чистая automation-поверхность (ADR-006), коммуницируется в техдоках разработчикам, не в продуктовом маркетинге.
- ❌ **Платный «stealth-tier»** — инвертирует экономику и создаёт стимул держать юзеров blocked-by-default.

### 9.6 Прозрачность

- **Network log в UI** (всегда видимый, Ctrl+Shift+N для деталей):
  - сколько запросов, куда, сколько байт, что заблокировано.
- **Permission UI** — каждое разрешение (камера/гео/нотификации) отдельным prompt, по умолчанию `deny`. Никаких «remember for this site» автоматически.
- **No silent network** — если что-то идёт во время idle (телеметрия, prefetch, update check), это видно и отключаемо.

### 9.7 Принципиальный отказ

- Никакой телеметрии, ни анонимной, ни «opt-in» по умолчанию.
- Никаких облачных аккаунтов в браузере.
- Никаких поисковых подсказок «из коробки» (опт-ин в настройках).
- Никаких «recommended extensions» магазинов.
- Никакой phone-home, кроме проверки обновлений (можно отключить).

### 9.8 Диагностика и crash reports

Расширение принципа №7: **диагностика — обязательно локальная, никогда не отправляется автоматически.** Это касается и crash dump-ов, и developer-log-ов, и performance-трейсов. Если что-то выходит наружу — только потому, что пользователь сам приложил файл к bug report.

**Три потока диагностической информации:**

| Слой | Кому | Где живёт | Видимость |
|---|---|---|---|
| Network log | Пользователю (real-time) | UI-панель (§9.6) | Всегда видна, Ctrl+Shift+N для деталей |
| Developer log | Разработчику / advanced user | stderr (по умолчанию); файл — только при явном `--log-file <path>` | По умолчанию `warn`+, фильтр через `LUMEN_LOG=lumen_network=debug` env var |
| Crash dump | Разработчику через пользователя | `<profile>/crashes/lumen_<timestamp>.log` (текстовый) | Никогда не отправляется автоматически. Пользователю показывается путь и фраза «приложите этот файл к issue» |

**Структура crash dump-а:**
- Версия Lumen, target triple, флаги сборки.
- Stacktrace (если доступен — Rust panic message + backtrace).
- **Последние 50 событий из `EventSink`** — даёт контекст «что делал браузер за миг до падения» без необходимости включать verbose-logging заранее. Это и есть причина, по которой `EventSink` (§9.6) — центральная подсистема, а не «опция».
- Содержимое open-tabs snapshot (URL + title, без cookies и form-state — последние утечь не должны).
- Список загруженных WASM-плагинов и их capability-токенов.

**`lumen --diagnose <path>` CLI:**
- Собирает версию, env, конфиг профиля (без секретов), последние N developer-log-ов, last crash dump в один txt-файл.
- **Не отправляет ничего.** Просто пишет файл и сообщает путь.
- Идиоматичный сценарий: пользователь натолкнулся на баг → `lumen --diagnose ~/lumen-bug.txt` → прикладывает к issue.

**Логирование как trait, не зависимость:**
- Свой минимум: `log!(level, target, "msg", k=v)` макрос пишет в стуб (stderr / файл / EventSink-наблюдателя), без `tracing` / `log` крейтов. ~200 строк, никаких новых dep.
- Через `EventSink::emit` идут структурированные события (`Request*`, `Tab*`, `Navigation`, `PageLoaded`); developer-log — отдельный поток для «плоских» сообщений (parser error, layout warning).
- Если потом упрёмся в необходимость span-trace для перформанса — пересмотрим, возможно tracing как exception #5.

**Дополнения к `EventSink` (см. §9.6):**
- ⬜ **`RequestFailed { tab_id, url, stage, reason }`** — событие для DNS / connect / TLS-ошибок **до** `RequestCompleted`. Сейчас invariant «Started без Completed = failure» неявный — observer не знает, где именно споткнулось. Добавление stage (`Dns` / `Tcp` / `Tls` / `Read`) сразу даёт user-facing объяснение в network log: «не удалось подключиться» vs «сертификат недействителен».
- ⬜ **Crash hook на `EventSink`** — последние 50 событий буферизуются in-memory; при panic-е дамп сохраняется в crash dump до завершения процесса.

**Чего НЕ делаем:**
- ❌ Sentry / Bugsnag / любые SaaS crash-aggregator-ы.
- ❌ Анонимный «opt-in» сбор статистики падений. Любая статистика — это телеметрия, см. §9.7.
- ❌ Автоматический «send report?» dialog. Только пользователь решает, что и куда отправлять.

---

## 10. Локализация и поддержка русского языка

Поддержка русского — first-class требование, не «потом». Контракт на каждом этапе разработки.

### 10.1 Кодировки

Старые RU-сайты часто отдают **Windows-1251** или **KOI8-R**, реже CP866. HTML parser определяет кодировку из `Content-Type`, `<meta charset>`, BOM или (в крайнем случае) байт-паттернов и конвертирует в UTF-8 на входе DOM. **Реализация — своя:** таблицы декодирования — это публичные данные, hand-rolled SIMD не нужен на старте. Trait — `EncodingDetector` в `lumen-core::ext`.

### 10.2 Шрифты

Font fallback chain обязательно содержит шрифты с кириллицей:

- **Windows:** Segoe UI, Tahoma, Arial.
- **macOS:** SF Pro, Helvetica Neue.
- **Linux:** Noto Sans, DejaVu Sans, Liberation Sans.

Fallback работает на каждый символ-сирота, не на всю строку (стандартное поведение `cosmic-text` + `rustybuzz`). Регрессионный тест: «Привет, мир» с Latin-only шрифтом должен показать кириллицу из fallback.

### 10.3 URL и IDN

Кириллические домены (`президент.рф`, `почта.рф`) — RFC 5890. В сетевом запросе → Punycode (`xn--...`), в UI → всегда Unicode. Защита от homograph-атак по правилам IDNA. **Crates:** `idna`, `unicode-security`.

### 10.4 Локаль `ru-RU`

- Дата: `12.05.2026` (dd.mm.yyyy).
- Время: 24-часовое, `14:30`.
- Числа: `1 234,56` (NBSP-разделитель тысяч, запятая для десятичных).
- Неделя начинается с понедельника.

**Crate:** `icu` (модульный, подключаем нужные компоненты).

### 10.5 Anti-fingerprinting vs язык

Tor Browser форсирует `Accept-Language: en-US,en` ради единого fingerprint — это ломает русскоязычный UX (получаешь английские версии сайтов). Lumen в strict-mode **НЕ** нормализует язык до английского, оставляет `ru,en;q=0.5`. Остальные fingerprint-метрики (timezone, screen, canvas, fonts) нормализуем как обычно. Это сознательный компромисс: UX > fingerprint resistance для одной метрики.

### 10.6 Поисковые движки

Встроенные опции, пользователь выбирает при первом запуске:

- DuckDuckGo,
- Brave Search,
- **Яндекс** — для русскоязычных,
- Mojeek,
- свой URL.

Без «облачных» подсказок по умолчанию — поиск только при Enter.

### 10.7 Сортировка и поиск по тексту

История, закладки, omnibox-поиск с кириллицей:

- collation по русскому алфавиту, не по Unicode codepoints,
- Ё↔Е equivalence (опционально),
- транслитерационный поиск: ввод `privet` находит «привет».

**Crate:** `icu_collator`.

### 10.8 UI-переводы

Русский — первый язык наравне с английским, не «после релиза». Формат **Fluent** (FTL, Mozilla) — корректная плюрализация (1 файл / 2 файла / 5 файлов), грамматические падежи. Дизайн UI учитывает: русский текст в среднем на ~30% длиннее английского, тулбары/диалоги не должны обрезаться.

### 10.9 Reader mode

Readability heuristics родом из английского. Регулярно тестируем на: Habr, ТАСС, Lenta, Meduza, КП. Возможна настройка порогов «main content vs sidebar» под кириллические тексты.

### 10.10 Перенос слов

✅ CSS `hyphens: auto` — реализовано (P1, 2026-05-29). `KnuthLiangHyphenation` в `lumen-encoding` реализует `HyphenationProvider` через provisional crate `hyphenation = "0.8"` (Knuth–Liang, TeX-словари). Поддерживает 11 локалей включая ru. Подключён в `lumen-shell` через `layout_measured_hyp`.

### 10.11 Тесты на RU-вебе

Отдельный CI-прогон по топу русскоязычных сайтов: Yandex, VK, OK, Mail.ru, Habr, Lenta, RT, ТАСС, Госуслуги. Скриншот-сравнение с Chromium как baseline. Отдельный от глобального топ-1000.

---

## 11. Модульность и расширяемость

Lumen строится из независимых модулей с явными интерфейсами. Это две связанные, но разные задачи: модульность собственного кода и поддержка сторонних плагинов.

### 11.1 Внутренняя модульность

Принципы:

- **Однонаправленные зависимости.** `lumen-core` — основание, на него опираются все остальные крейты. Никаких циклов. Каждый крейт зависит только от «ниже» по уровню.
- **Стабильные публичные API.** Каждый крейт экспортирует узкий публичный интерфейс (как правило, `trait` + базовые типы). Внутренности — `pub(crate)`.
- **Cargo features.** Опциональные подсистемы за feature gates: `v8`, `quickjs`, `webgl`, `ru-hyphenation`, `tor`. По умолчанию минимальный набор.
- **Базовый крейт `lumen-core`.** Общие типы: `Url`, `MimeType`, `Error`, `EventBus`, `Capability`. Всё, что нужно более чем одному модулю, живёт здесь.

Точки расширения для собственного кода (через `trait` в `lumen-core` или соседних crates):

| Trait | Назначение | Возможные реализации |
|---|---|---|
| `JsRuntime` | мост к JS-движку | QuickJS, V8, SpiderMonkey, mock |
| `StorageBackend` | БД для cookies / IndexedDB | redb, sqlite, in-memory |
| `NetworkTransport` | HTTP-стек | свой HTTP/1.1, /2 (Phase 1+); mock для тестов |
| `RenderBackend` | растеризация | свой CPU-rasterizer (Phase 0), свой GPU-pipeline поверх wgpu (Phase 1+); headless для тестов |
| `EncodingDetector` | определение кодировки HTML | свой по байт-таблицам (cp1251, KOI8-R, CP866, UTF-8, ASCII, Win-1252) |
| `WindowingBackend` | OS event loop + окна | winit (exception, см. §5); потенциально свой нативный — Phase 3+ |
| `TlsBackend` | TLS + crypto | rustls (exception, см. §5); потенциально системный (SChannel / Network.framework) |
| `SearchProvider` | поисковая система | DuckDuckGo, Brave, Яндекс, кастомный |
| `FilterListSource` | источник списков рекламы | EasyList, локальный файл, OTA-канал |
| `FontProvider` | поиск шрифтов | системный, bundled, веб |

Каждый trait — точка для будущей замены без правки потребителей.

### 11.2 Сторонние плагины

Три реальных архитектурных пути:

| Подход | Плюсы | Минусы | Примеры в индустрии |
|---|---|---|---|
| **WASM** через wasmtime | Песочница из коробки, кросс-язычность, capability-based security, стабильный ABI (WASI 0.2) | Медленнее native (но достаточно для не-hot path), runtime ~5 МБ | Zed, Figma, Envoy, Shopify Functions |
| **Native dylib** | Максимальная скорость, прямой доступ к API | Полное доверие, нестабильный Rust ABI, частый источник crashes/CVE | Bevy plugins |
| **WebExtensions (JS)** | Огромная экосистема (uBO и др.) | Привязка к JS-движку, сотни `browser.*` API, всё через JS | Firefox, Chrome, Safari |

**Рекомендация: WASM через `wasmtime`.** Почему для privacy-first браузера это правильный выбор:

- Плагин **по умолчанию не имеет доступа** к ФС, сети, других вкладок, cookies, истории. Хост (Lumen) выдаёт capability tokens на конкретные операции.
- Плагин можно писать на **любом языке**, компилируемом в WASM (Rust, Go, AssemblyScript, TS-через-AS).
- WASI 0.2 и Component Model — общепринятый стандарт, не маргинальная экзотика.
- Производительность для плагинов приемлема — они не на критическом пути рендера.

Альтернативы оставляем не закрытыми: подмножество WebExtensions API можно реализовать поверх WASM-инфраструктуры в виде плагина-шима, который транслирует `browser.*` JS-вызовы в capability-вызовы.

### 11.3 Plugin API — черновик

Что плагин **может**:

- Подписываться на события: `tab_created`, `tab_closed`, `page_loaded`, `request_intercepted`, `key_pressed`, `selection_changed`.
- Регистрировать команды в команд-палитре (Ctrl+Shift+P).
- Регистрировать пункты в контекстном меню (правый клик).
- Рисовать UI в выделенном rect сайдбара (своя «вкладка» в боковой панели).
- Получать выделенный текст, манипулировать им.
- Делать сетевые запросы — только если выдан capability `network` с whitelist доменов.
- Читать/писать в свой namespace `KV`-хранилища.

Что плагин **НЕ может**:

- Менять движок рендера, парсер, layout.
- Читать cookies / storage других сайтов без явного `storage:<origin>` capability.
- Запускать произвольный код на хосте, лезть в чужие плагины.
- Постоянно «висеть» в фоне без причины — runtime ограничивает CPU/память.

### 11.4 Capability-модель (вместо «разрешений» Chrome)

В Chrome/Firefox у плагина есть статический список permissions в манифесте; пользователь видит «доступ ко всем сайтам». Это устарело. В Lumen:

- Плагин при установке заявляет **категории** capabilities (network, storage, clipboard, UI-sidebar).
- При первом использовании каждой capability — runtime prompt с конкретикой («плагин X хочет послать запрос на api.example.com — разрешить раз / всегда / запретить»).
- Capability можно отозвать в любой момент.
- Список выданных capability-tokens видно в UI настроек плагина.

### 11.5 Этапность

- **Phase 0–1:** внутренняя модульность, `lumen-core`, основные traits как точки замены. Никаких сторонних плагинов.
- **Phase 2:** первая версия Plugin API + wasmtime host. Один-два дев-плагина для проверки (например, sidebar для заметок, кастомный adblock-провайдер).
- **Phase 3+:** расширенный capability-набор, дистрибуция через self-hosted manifests + minisign-подписи. Никакого централизованного «магазина» с reviewers (как Chrome Web Store) — пользователь сам решает, кому доверять.

---

## 12. Knowledge layer и уникальные фичи Lumen

Раздел фиксирует функциональность, которой нет в массовых браузерах не из-за технической сложности, а из-за конфликта интересов их вендоров (Google, Microsoft зарабатывают именно на том, что эти фичи отсутствуют). Lumen, не имея рекламной модели и облачных сервисов, закрывает эти пробелы first-class.

Архитектурно эти фичи живут в новом крейте `lumen-knowledge` (хранение + индексация), опциональном `lumen-ai` (локальные эмбеддинги + RAG) и UI-расширениях `lumen-shell` (omnibox-фильтры, боковые панели).

### 12.1 Полнотекстовый поиск по истории

**Что:** omnibox ищет не только по URL и заголовкам, но по полному содержимому всех ранее посещённых страниц.

**Почему:** классическая боль *«найди ту статью про переработку лития, что я читал в марте»*. Chrome намеренно не делает — это конфликт с поиском Google.

**Реализация:**
- При навигации фоновый readability-extract извлекает основной текст без UI-шума (то же ядро, что в §10.9 reader-mode).
- Текст идёт в **SQLite FTS5** виртуальную таблицу (exception #5 из §5): tokenizer `unicode61 remove_diacritics 2`, опциональный кастомный tokenizer для кириллицы (ё↔е, Porter-stemmer для русского — см. §10). Ранжирование через встроенный bm25() в FTS5.
- Схема: `history(id INTEGER PK, url TEXT, title TEXT, visit_date INTEGER, favicon_hash BLOB, text_sha256 BLOB)` + `history_text(rowid, text)` (FTS5 virtual). Связь через `rowid = history.id`.
- Объём: средняя текстовая статья ~10 КБ; 100 000 страниц ≈ 1 ГБ; SQLite FTS5 со сжатием prefix-blocks ~400 МБ. Лимит по диску настраивается (по умолчанию 500 МБ → авто-вытеснение старого триггером).
- Запрос: omnibox с префиксом `@history` или просто текст — результаты из истории / заметок / закладок выше внешнего поиска. `SELECT ... WHERE history_text MATCH ? ORDER BY bm25(history_text) + (julianday('now') - julianday(visit_date)) / recency_decay LIMIT 20`.

**Локализация:** FTS5 `unicode61` нормализует регистр по Unicode; ё↔е и Porter-stemmer реализуем как custom tokenizer (FTS5 supports external tokenizers через C-callback — пока что отложим до Phase 2 и используем дефолтный unicode61).

**Фаза:** 2 (после HTTP-клиента, когда есть смысл накапливать историю).

### 12.2 Аннотации и заметки

**Что:** выделил текст на странице → команда «сохранить как заметку с контекстом». Заметка хранит выделенный фрагмент, окружающий абзац, URL, дату, опциональный комментарий пользователя.

**Почему:** замена внешних сервисов (Readwise, Hypothesis, Notion Web Clipper, Obsidian). Это базовая для читателя функциональность, которой нет встроенной нигде.

**Реализация:**
- Selection / Range API в DOM (стандартный, нужен и для других целей — поиск по странице, copy-to-clipboard).
- Context-menu действие в shell + горячая клавиша.
- Хранение — в той же `lumen-knowledge` БД, индексируется тем же FTS из §12.1.
- При повторном открытии страницы заметки восстанавливаются поверх DOM как highlight-наложения (опционально включается).
- Экспорт всех заметок в Markdown / JSON — кнопка в Notes panel.
- Per-profile (заметки личного профиля не видны в рабочем).

**Фаза:** 2.

### 12.3 Read-later / офлайн-чтение

**Что:** кнопка «сохранить страницу офлайн». Берёт полный snapshot — DOM + CSS + изображения — и кладёт в profile-каталог. Дальше страница доступна без сети сколь угодно долго.

**Почему:** замена Pocket / Instapaper. Базовая функциональность читателя — должна быть встроенной, а не подписочной.

**Реализация:**
- При сохранении: walk текущий DOM, скачиваем все ресурсы (`<img>`, `<link rel=stylesheet>`, inline-background-image из стилей), сохраняем как single-file HTML (data-URI inline) или связанный набор файлов.
- Per-profile квота по диску (по умолчанию 1 ГБ), настраивается; FIFO-вытеснение по дате доступа.
- Список «Read Later» в боковой панели shell; клик открывает локально без сети с пометкой «офлайн-копия от \<дата\>».
- Текст офлайн-копий тоже идёт в индекс §12.1.
- Опционально: одноразовое чтение из RSS / Atom-фидов (тоже офлайн).

**Фаза:** 2.

### 12.4 Поиск по содержимому открытых вкладок

**Что:** omnibox с префиксом `@tabs` ищет среди *сейчас открытых* вкладок (не истории) по содержимому — title, видимый текст, форма URL. Удобно, когда открыто 50 вкладок и не вспомнить, какая нужна.

**Почему:** один из самых частых запросов на форумах. Edge / Arc частично закрыли. Chrome / Firefox — нет.

**Реализация:**
- Live-индекс открытых вкладок (subset §12.1 механики, но без диск-persistence).
- Учитывает hibernated вкладки тоже (по сохранённому DOM-snapshot).
- Фильтр по workspace / profile.

**Фаза:** 2.

### 12.5 Локальный AI layer (опциональный)

**Что:** маленькая локальная модель + локальный embedding для:
- **Семантического поиска** по истории / заметкам / закладкам. *«Что я читал про электрокары»* находит даже если в статье нет слова «электрокар», но смысл совпал.
- **Суммаризации** страницы по запросу (никаких облачных API).
- **Q&A над собственной историей** (RAG): «какие источники говорили про X».

**Почему:** индустрия идёт в облачные ИИ-агенты (Atlas от OpenAI, Comet от Perplexity, Dia, Edge Copilot). У них три фундаментальные проблемы: приватность утекает наружу, дорого по токенам, prompt-injection как класс уязвимостей. Локальная модель решает все три.

**Реализация:**
- Отдельный крейт `lumen-ai`, под Cargo feature-флагом `ai`. По умолчанию **выключен** в bundle (бинарь Lumen без AI меньше и проще).
- Backend через HTTP API уже установленной Ollama (если есть) — нулевая интеграция, дёшево. Альтернатива: встроенный llama.cpp через FFI — это потенциально **5-е exception** в §5 с обоснованием. Решение откладываем до момента включения модуля.
- Эмбеддинги (`bge-small`, `nomic-embed-text` или подобное) предвычисляются при индексации страницы (§12.1) если модуль включён.
- Векторный store: HNSW-индекс в `lumen-knowledge` — приближённый ближайший сосед за O(log N).
- UI: команда `@ai` в omnibox или отдельная панель «Ask Lumen».
- Capability `local-ai` для плагинов: WASM-плагин может запросить эмбеддинг или генерацию через Lumen-runtime, никаких сетевых вызовов.

**Фаза:** 3+. Не критичная, но потенциально killer-feature. Phase 0-2 работает без AI.

### 12.6 Focus mode

**Что:** режим, в котором браузер активно снижает когнитивную нагрузку:
- Скрыты боковые панели, badges, нотификации.
- Фоновые вкладки автоматически hibernated (агрессивнее обычной гибернации).
- Reader mode принудительный для текстовых страниц.
- Один таб виден за раз, минимальный chrome.
- Опционально: Pomodoro-таймер; по окончании цикла — нотификация.

**Почему:** ни один массовый браузер не помогает пользователю фокусироваться — все оптимизированы на engagement (engagement = time-on-platform = реклама). Lumen без рекламной модели может прямо помогать пользователю выйти из браузера.

**Реализация:** UI feature поверх существующей инфраструктуры. Не требует новых крейтов. Toggle через команду или горячую клавишу.

**Фаза:** 2.

### 12.7 Tab session export / import

**Что:** сериализация набора открытых вкладок (включая дерево, workspaces, scroll-позиции, базовые значения форм) в файл; импорт восстанавливает сессию.

**Почему:** переезд между компьютерами, бэкап перед переустановкой, шаринг рабочей сессии с коллегой. Все хотят, никто не делает в полном виде.

**Реализация:**
- Формат: компактный JSON или TOML (бинарный не нужен).
- Поля: URL, title, scroll position, form values (textarea / input), parent в дереве, workspace.
- Импорт: lazy — вкладки восстанавливаются как hibernated, активируются по клику.
- Cross-profile (можно экспортировать рабочий и импортнуть в личный с подтверждением).

**Фаза:** 1-2 (легко, можно сделать рано).

### 12.8 Семантические закладки

**Что:** вместо «сохрани ссылку» — *«сохрани смысл, напомни связанное»*. Закладка содержит автоматическую суммаризацию + теги + эмбеддинг. При релевантных omnibox-запросах закладка всплывает сама.

**Почему:** обычные закладки = «складировал и забыл». Семантические превращают коллекцию в активный граф знаний.

**Реализация:** расширение §12.2 + §12.5:
- Суммаризация через локальный AI (если модуль включён) или вручную пользователем.
- Эмбеддинг суммаризации хранится рядом с закладкой.
- Поиск похожих — cosine similarity на эмбеддингах при текущем omnibox-запросе.
- Без AI-модуля — обычные tag-based закладки (теги вручную).

**Фаза:** 3 (зависит от §12.5).

### 12.9 Граф знаний пользователя

**Что:** интерактивная визуализация связей между прочитанными страницами / заметками / закладками. Темы, домены, пересечения, кластеры.

**Почему:** *«что я знаю про X»*, *«какие источники мне доверять по теме Y»*, *«какие темы давно не трогал»* — таких инструментов нет в браузерах в принципе. Для пользователей, активно работающих со знанием (исследователи, журналисты, аналитики).

**Реализация:**
- Граф строится на данных §12.1-12.5: узлы — страницы / заметки / закладки; рёбра — by-domain / by-tag / by-semantic-proximity (через AI-эмбеддинги) / by-link-citation.
- Render — SVG или Canvas с force-directed layout.
- Интерактив: фильтры по дате / тегам / профилю, поиск, drill-in.
- Опционально: экспорт в формат для Obsidian / Roam Research.

**Фаза:** 3+. Опционально.

### 12.10 Кастомизация UI

**Что:** пользователь переаранжирует toolbar, скрывает / показывает панели, выбирает темы, настраивает omnibox-поведение. По духу — Firefox 2008, до того как массовые браузеры стали неконфигурируемыми.

**Почему:** один из самых частых запросов на форумах. Vivaldi нишево, Chrome / Edge / Safari почти не дают. Кастомизация — не «advanced опция», а право пользователя на свой инструмент.

**Реализация:**
- Все UI-блоки (toolbar, sidebar, status bar, omnibox) — переставляемые drag&drop.
- Темы: JSON с цветовой схемой + опциональные CSS-оверрайды (для chrome-UI, не для страниц).
- Конфиг в `~/.config/lumen/ui.toml`, edit-able руками или через Settings.
- Плагины (§11) могут добавлять свои UI-блоки в любую панель.

**Фаза:** 2-3.

### 12.11 Кросс-устройственная синхронизация (E2E)

**Что:** опциональная синхронизация состояния (вкладки, история, закладки, заметки, скролл-позиция, форма) между устройствами с end-to-end шифрованием. Self-hosted сервер или peer-to-peer.

**Почему:** *«начал читать в метро на телефоне → пришёл домой, открыл ноут, продолжается с того же места»*. Safari ближе всех в этом, но только в Apple-экосистеме.

**Реализация:**
- Self-host сервер: маленький HTTP-сервис-релей, который не видит содержимого — только зашифрованные blob-ы.
- Шифрование: X25519 + AES-GCM, ключи производятся из паролей профилей через Argon2id KDF.
- Альтернатива (без сервера) — peer-to-peer через LAN / Tailscale.
- НЕ строим централизованный облачный сервис «Lumen Sync» — это против философии (см. §1).

**Фаза:** 3+. Mobile-клиент необходим для real use-case, что упирается в mobile из §16 фаз.

### 12.12 DevTools (инспектор / консоль / network)

**Что:** встроенный инструмент разработчика — DOM-инспектор, computed styles, JS-консоль, network panel, performance trace. Не аналог Chrome DevTools по объёму, а минимально жизнеспособный набор для отладки страниц в самом Lumen.

**Почему:** не уникальная фича — есть во всех браузерах. Но без неё Lumen воспринимается как «недо-браузер»: невозможно отладить, почему страница рендерится не так, как ожидается, не переключаясь в Chrome. Также критично для собственной разработки движка — встроенный инспектор раньше станет dogfooding-инструментом, чем внешним пользовательским.

**Реализация:**
- Отдельная панель в shell (toggle через F12).
- DOM tree — переиспользует существующий `lumen-dom::Node` (read-only view).
- Computed styles — экспозит `ComputedStyle` из `lumen-layout::style` как структурированный JSON.
- Box model overlay — рисуется через display list (margin/border/padding/content). Использует ту же геометрию, что и paint.
- Network panel — слушает события из `NetworkTransport` trait (`lumen-core::ext`).
- JS-консоль — поверх `JsRuntime` trait (rquickjs/V8), eval в контексте страницы.
- Полностью локально, никакой удалённой отладки в Phase 0-3.

**Фаза:** 4+. Не блокирует ни одну из §12.1-12.11. До этого момента отладка через `--dump-layout` / `--dump-display-list` и логи `tracing` (см. CLAUDE.md «Dump modes»).

### 12.13 Tab UX (вертикальные / tree-style / workspaces / split view)

**Что:** современная модель управления вкладками — вертикальная панель, дерево «родитель-потомок», workspaces (Arc Spaces), split view, авто-архивирование по возрасту.

**Почему:** один из топ-запросов 2024-2026. Arc сделал mainstream, Edge/Vivaldi/Zen догоняют. Без этого Lumen выглядит как «браузер из 2010-х».

**Подзадачи:**
- **13.1** `[P3]` Vertical tabs panel (toggle, drag-reorder, collapse) — `shell/src/tabs/`
- **13.2** `[P3]` Tree-style tabs: parent-child relations в TabModel + UI отступы — `shell/src/tabs/tree.rs`
- **13.3** `[P3]` Workspaces (несколько изолированных групп вкладок, переключение хоткеем) — `shell` + persistence в `lumen-storage::workspaces`
- **13.4** `[P3+P2]` Split view: 2-4 страницы на одном окне в tiling-режиме. **P3** — UI и tab-routing; **P2** — несколько viewport-ов в одном wgpu surface (или multi-surface)
- **13.5** `[P3]` Tab auto-archive: hibernate вкладок старше N часов, восстановление по клику — `shell/src/tabs/archive.rs`

**Фаза:** 2.

### 12.14 Power-user input (vim-keys, gestures, omnibox-алиасы, regex find)

**Что:** keyboard-first навигация (Vimium-стиль), mouse gestures, кастомные алиасы в omnibox (`y!` → YouTube), find-in-page с regex.

**Почему:** стабильно нишевый, но фанатичный спрос. Дёшево по реализации, высокая лояльность аудитории.

**Подзадачи:**
- **14.1** `[P3]` Vim-style key bindings (modal: normal/insert, биндинги `j/k/gg/G/f`) — `shell/src/input/vim.rs`
- **14.2** `[P3+P1]` Click-hint overlay (буквенные подсказки на ссылки/кнопки). **P3** — рисование overlay и keypress handling; **P1** — итератор по clickable-элементам с экранными координатами в `lumen-layout`
- **14.3** `[P3]` Mouse gestures (capture drag-trail, распознавание паттернов: ← закрыть, → вперёд, ↓ новая вкладка) — `shell/src/input/gestures.rs`
- **14.4** `[P3]` Custom omnibox aliases (пользователь добавляет `gh` → `https://github.com/search?q=%s`) — `shell` + storage в user config
- **14.5** `[P3+P1]` Find-in-page с regex. **P3** — UI поисковой строки и highlight через display list overlay; **P1 done** — `collect_visible_text(&LayoutBox) -> Vec<TextFragment>` в `lumen-layout::text_iter` (10 тестов)

**Фаза:** 2-3.

### 12.15 Privacy UX (блокировщик с UI, per-site контролы, cookie-banner dismiss)

**Что:** встроенный блокировщик трекеров/рекламы с пользовательским UI (как Brave Shields), панель per-site разрешений (cookies / JS / images), автоматическое скрытие cookie-баннеров.

**Почему:** после Manifest V3 (2024-2025) Chrome подрезал uBlock Origin — спрос на встроенное блокирование вырос радикально. Cookie-баннеры всех замучили (Consent-O-Matic популярен).

**Подзадачи:**
- **15.1** `[P3]` Block list engine: parser EasyList (ABP-syntax) + hosts-files + persistence — `lumen-network::filter` (есть trait `RequestFilter`, нужна реальная реализация)
- **15.2** `[P3]` Per-site permission UI: панель с тогглами cookies/JS/images/autoplay; per-site overrides в storage — `shell/src/site_settings/`
- **15.3** `[P3]` Cookie-banner auto-dismiss: список CSS-селекторов из I-don't-care-about-cookies + heuristic. Применяется через UA-stylesheet injection или JS-snippet через `JsRuntime` — `shell/src/cookies/banner.rs`
- **15.4** `[P3]` Shields-style toolbar widget: счётчик заблокированных запросов + быстрый toggle для домена — `shell/src/toolbar/shields.rs`

**Фаза:** 2.

### 12.16 Web platform baseline (Passkeys, контейнеры, sidebar web panels)

**Что:** функциональность, которая в 2024-2026 стала «обязательной для современного браузера» — иначе сайты ломаются или UX деградирует.

**Почему:** Passkeys/WebAuthn — стандарт авторизации с 2024 (Apple/Google/MS), без поддержки часть сайтов недоступна. Контейнеры (Firefox-style isolation) решают «два gmail-аккаунта». Sidebar web panels (Edge/Vivaldi) — растущий UX-паттерн.

**Подзадачи:**
- **16.1** `[P3]` Passkeys / WebAuthn: CTAP2-клиент (USB/NFC/internal), новый trait `CredentialProvider` в `lumen-core::ext`, JS-API `navigator.credentials` — `lumen-network::webauthn` + `crates/js/src/credentials.rs`
- **16.2** `[P3]` Tab containers: storage partitioning по контейнеру (отдельные cookie/localStorage namespaces); UI выбора контейнера для вкладки — `lumen-storage::partition` + `shell`
- **16.3** `[P3]` Sidebar web panels: вторая мини-страница в боковой панели (например, чат/мессенджер), независимый рендер-таргет — `shell/src/sidebar/web_panel.rs` + использует существующий рендер-pipeline

**Фаза:** 2-3.

### 12.17 Где это всё трогает архитектуру

Новые крейты:
- **`lumen-knowledge`** — FTS-индекс, аннотации, read-later каталог, хранение в KV-store (`lumen-storage`).
- **`lumen-ai`** (опционально, feature-flag) — embedding pipeline, HNSW-индекс векторов, мост к локальному LLM-backend.

Новые trait-точки расширения в `lumen-core::ext`:
- **`KnowledgeStore`** — абстракция с FTS-методами (insert / search / delete).
- **`AiBackend`** (опционально) — `embed(text) → Vec<f32>`, `generate(prompt, context) → Stream`.

UI расширения в `lumen-shell`:
- Omnibox-префиксы: `@history`, `@notes`, `@tabs`, `@bookmarks`, `@ai`, `@read-later`.
- Боковая панель «Knowledge» с разделами: Notes, Read Later, Bookmarks, Knowledge Graph.
- Context-menu действие «Save as note» в paint-слое.
- Focus-mode toggle в shell.

Capability-модель плагинов (§11.4) расширяется:
- **`KnowledgeRead`** — читать историю / заметки / закладки текущего профиля.
- **`KnowledgeWrite`** — добавлять / редактировать заметки / закладки.
- **`LocalAi`** — запрашивать embed / generate через локальный AI.

---

## 13. Безопасность

### 10.1 Sandboxing

- **Linux:** seccomp-bpf фильтр (whitelist syscalls), user namespaces, дополнительно Landlock для FS.
- **macOS:** App Sandbox через `sandbox_init`, entitlements в plist.
- **Windows:** AppContainer + Job Object + Restricted Token + Mitigation Policies (DEP, ASLR, CFG).

Каждый renderer-процесс — в своём сэндбоксе, без доступа к сети (только через IPC к network service) и без доступа к диску (только через IPC к storage service).

### 10.2 Memory safety

- Rust исключает 70% типичных CVE (use-after-free, buffer overflow, data races).
- `unsafe` — только в:
  - FFI к JS-движку (V8/QuickJS) — `engine/js-binding`,
  - FFI к декодерам, если используем C-либы (AVIF),
  - кастомных аренах DOM (когда индексы выходят за рамки borrow checker).
- Все `unsafe`-блоки помечены, документированы, ревью обязательно.
- `cargo-geiger` для мониторинга `unsafe` в зависимостях.

### 10.3 Process isolation

- Site isolation по eTLD+1.
- COOP / COEP / CORP — поддерживаем.
- `SharedArrayBuffer` — только с правильными заголовками (защита от Spectre).
- Process per origin для opaque origins (`data:`, sandboxed iframes).

### 10.4 Updates

- Подписанные релизы (minisign или sigstore).
- Update-проверка раз в день (можно отключить), не загружает ничего без согласия (или авто-загрузка, как опция).
- Roadmap — детерминированные сборки (reproducible builds) к 1.0.

### 10.5 Дополнительно

- CSP, Mixed Content, Subresource Integrity — строгие дефолты.
- HSTS preload list — встроенный, обновляемый.
- Certificate transparency — проверяем SCT.
- Safe Browsing — **НЕ используем Google API**. Опционально подключаем собственный список через DNS (например, Quad9 уже блокирует malware).
- Fuzzing: `cargo-fuzz` на HTML parser, CSS parser, image decoders, URL parser, JS-binding границы. Запуск в CI.

---

## 14. Производительность

### 11.1 Цели

| Метрика | Цель v0.1 | Цель v1.0 |
|---|---|---|
| Cold start до окна | < 300 мс | < 500 мс |
| Cold start до загруженной google.com | n/a | < 1.5 с |
| RAM на пустую вкладку | < 50 МБ | < 80 МБ |
| RAM на 5 типичных вкладок | < 250 МБ | < 600 МБ |
| RAM на 100 hibernated вкладок | < 200 МБ | < 300 МБ |
| Speedometer 3.0 | n/a | в пределах 2× от Chromium |
| Идл CPU (видимое окно) | < 1% | < 1% |

### 11.2 Стратегии

- **Параллельный layout / style** через `rayon` — главный архитектурный плюс перед Blink (Blink в этом плане монолитен).
- **Lazy tabs** — при восстановлении сессии вкладки не загружаются.
- **Tab hibernation** — освобождение renderer-процесса с сохранением навигации.
- **GPU-композитинг** — всё на wgpu.
- **Кэширование** — display list, computed styles переиспользуются при инвалидации.
- **Инвалидация** — точечная, не «пересчитать всё дерево».
- **Image decoding** — на отдельных тредах, прогрессивный.

### 11.3 Профилирование

- `tracy` интегрирован, активируется флагом `--profile`.
- Бенчмарки в CI: layout простой страницы, парсинг HTML 10 МБ, JS Speedometer.
- Tracking регрессий — графики по коммитам.

### 11.4 Memory budget per tab — пятитайерная модель ([ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md))

Главный продуктовый дифференциатор Lumen наряду с приватностью — **RAM-нагрузка на вкладку**. Цель: 50 открытых вкладок в Lumen занимают ~400 MB, в Chrome — 6-10 GB. Достигается за счёт явной модели жизненного цикла вкладки с пятью tier'ами и тремя структурными инвариантами на подсистемы.

#### Tier'ы T0–T4 и переходы

| Tier | Когда | Что в RAM | Бюджет (v0.1) |
|---|---|---|---|
| **T0 Active** | foreground, видимая | JS heap, DOM, layout, paint, image cache, GPU textures | 80-200 MB |
| **T1 Background-recent** | скрыта < 5 мин | JS heap paused, остальное retained | 40 MB |
| **T2 Background-old** | скрыта 5-30 мин | JS heap → snapshot на диск, image/GPU cache drop, layout retained | 15 MB |
| **T3 Hibernated** | скрыта >30 мин или memory pressure | DOM → сериализован в SQLite; в RAM только TabMetadata (URL, title, scroll, favicon) | 200 KB |
| **T4 Closed-recoverable** | закрыта пользователем | 0 RAM (entry в session history) | 0 |

Переходы между tier'ами — **OR трёх условий**: idle timeout + OS memory pressure + LRU within global budget. Pinned вкладки не уходят за T1 (явный пользовательский opt-in).

#### Restore SLO (binding)

| Переход | Цель |
|---|---|
| T1 → T0 | ≤ 50 ms (resume JS event loop) |
| T2 → T0 | ≤ 200 ms (restore JS heap + re-decode visible images) |
| T3 → T0 | ≤ 1500 ms (deserialize DOM, re-run scripts, full layout+paint) |
| T4 → T0 | network-bound (fresh navigation) |

Регрессия > 20% на любом переходе — release-blocker (см. `lumen-bench` RAM-axis, задача 9G.3).

#### Три структурных инварианта (binding на subsystems)

Эти инварианты **должны быть приняты до Phase 1 finalize** соответствующих крейтов, иначе ретрофит обойдётся в 5-10× по часам (см. ADR-008 «Context»).

1. **DOM = arena с `NodeId(u32)`, не `Rc<RefCell<Node>>` граф.** Сериализуется через `bincode` для T3. `lumen-dom` уже движется в эту сторону — ADR делает это формально-обязательным.
2. **JsRuntime поддерживает `suspend()` / `resume()` / `pause()` / `unpause()`** через `lumen-core::ext::JsRuntime` trait. QuickJS это умеет, V8 — нет out-of-the-box. **Закрепляет QuickJS как обязательный Phase 0-2 выбор**; миграция на V8 в Phase 3 (ADR-004) допустима только при доказанной возможности suspend через V8 snapshot API.
3. **Layout и paint — pure functions от `(DOM, stylesheet, viewport)`.** Никаких `static MUT`, никаких lazy_static / OnceCell в `lumen-layout` / `lumen-paint`. T2→T0 = просто пере-вызов функции. Исключение — cross-tab кэши (glyph atlas, font metrics, image decode) живут в своих крейтах с явным eviction API.

#### Техники экономии на активной вкладке (T0)

Не отложены на hibernation — работают **постоянно** для уменьшения T0:

- **Image decode cache LRU + viewport-gating.** Декодировать только то, что в viewport ± buffer. При скролле — decode/discard. `1920×1080 RGBA = 8 MB`; страница с 30 картинками без gating = 240 MB только на изображениях.
- **GPU layer LRU + texture recycling.** Off-viewport stacking contexts освобождают свои textures когда удалены от viewport больше N экранов.
- **Glyph atlas LRU eviction.** Атлас не растёт безгранично; редко используемые глифы вытесняются.
- **JS heap GC tuning.** QuickJS GC thresholds настраиваются per-tab; pinned tabs получают более мягкий GC, идлящие — более агрессивный.
- **`MemoryPressureSource` trait** (`lumen-core::ext`) ✅ — слушает OS-сигналы (Win32 `GlobalMemoryStatusEx`, Linux PSI `/proc/pressure/memory`, macOS `host_statistics64(HOST_VM_INFO64)`) и эмитит `Low / Medium / High` события. Подсистемы (caches, GPU layers, decoders) подписываются.

#### Сводные RAM-targets

Расширение §14.1 (binding numbers vs `bench/baseline.json`):

| Сценарий | Soft v0.1 | Hard v0.1 | Soft v1.0 | Hard v1.0 |
|---|---|---|---|---|
| T0 simple page (samples/page.html) | 80 MB | 100 MB | 150 MB | 200 MB |
| T0 heavy page (samples/heavy.html, Habr-style) | 150 MB | 200 MB | 250 MB | 350 MB |
| T1 per tab | 40 MB | 60 MB | 60 MB | 100 MB |
| T2 per tab | 15 MB | 25 MB | 25 MB | 40 MB |
| T3 per tab | 200 KB | 1 MB | 200 KB | 2 MB |
| 50 вкладок (1 active, остальные mixed T1/T2/T3) | 400 MB | 600 MB | 800 MB | 1200 MB |

---

## 15. Тестирование

### 15.1 Пирамида тестов

Lumen использует пять уровней с разной стоимостью и зоной ответственности. Чем выше уровень — тем дороже и реже запуск, тем шире зона покрытия. Реализация автоматизации через `lumen-driver` (§6.11, [ADR-006](docs/decisions/ADR-006-automation-api.md)) — обязательная база для уровней 2-4.

```
┌────────────────────────────────────────────────────────────┐
│ 5. Top sites / WPT — раз в релиз     ~минуты на тест       │
├────────────────────────────────────────────────────────────┤
│ 4. Cross-browser vs Edge — ночной job ~секунды на тест     │
├────────────────────────────────────────────────────────────┤
│ 3. Snapshot pixel in-process — на PR  ~миллисекунды        │
├────────────────────────────────────────────────────────────┤
│ 2. Structural asserts (via lumen-driver) — на cargo test   │
│                                       ~миллисекунды        │
├────────────────────────────────────────────────────────────┤
│ 1. Unit + парсер-тесты — на cargo check ~микросекунды      │
└────────────────────────────────────────────────────────────┘
```

#### Уровень 1 — Unit-тесты и парсер-тесты

- `cargo test` per-crate. Inline `#[test]` + integration tests в `tests/`.
- Парсер-тесты: `html5lib-tests` для HTML, WPT-style для CSS.
- ✅ **Display-list snapshot tests** (legacy уровень 1.5): `serialize_display_list` + 6 golden-файлов в `lumen-paint/tests/snapshots/`. `UPDATE_SNAPSHOTS=1` для регенерации. Остаётся как тонкий слой между unit и in-process pixel snapshot.

#### Уровень 2 — Structural asserts через `lumen-driver` (новое, основной слой)

Через `BrowserSession` trait (§6.11) тест получает структуры **прямо из движка**, без процесса/окна/пикселей. Локализация бага — до поля в `ComputedStyle` или координаты `LayoutBox`.

```rust
#[test]
fn test_05_margin() {
    let mut s = InProcessSession::new();
    s.navigate("file://graphic_tests/05-margin.html");

    let box1 = s.layout_box("#box1").unwrap();
    assert_eq!(box1.margin.top, 16.0);
    assert_eq!(box1.border_box.width, 200.0);

    let style = s.computed_style("#box1");
    assert_eq!(style.background_color, Color::rgb(0xff, 0x00, 0x00));

    let tree = s.a11y_tree();
    assert_eq!(tree.find_by_role("button").unwrap().name, "Submit");
}
```

Бегает на каждый `cargo test` (миллисекунды). Не зависит от шрифтов, GPU, антиалиасинга, ОС.

#### Уровень 3 — In-process pixel snapshot

`session.screenshot()` рендерит в off-screen surface, возвращает `Image` в RAM. Сравнение с PNG-эталоном в `graphic_tests/snapshots/`. Никакого ffmpeg, gdigrab, title bar offsets, calibration TEST-00 — буфер байт-точный.

Для кросс-OS детерминизма (избежать ±1 LSB от GPU драйверов) — software rasterizer (`tiny-skia`, opt-in dep) под `cfg(test)`. См. ADR-006 «Consequences → tiny-skia».

```rust
#[test]
fn test_05_margin_visual() {
    let mut s = InProcessSession::new();
    s.navigate("file://graphic_tests/05-margin.html");
    assert_snapshot!(s.screenshot(), "05-margin.png");
}
```

Файл-эталон коммитится в репо (`graphic_tests/snapshots/*.png`). При несовпадении тест сохраняет `*.actual.png` и `*.diff.png` рядом. Обновление: `cargo test --update-snapshots` (помечается в PR описании).

#### Уровень 4 — Cross-browser vs Edge

Текущая схема (`graphic_tests/run.py`) сохраняется, **но переходит в отдельный ночной CI-job** — не основной gate. Цель — обнаружение «оба дня неправильно одинаково» (когда уровень 3 не ловит, потому что snapshot закрепил баг). Edge как внешний якорь.

#### Уровень 5 — Top 1000 sites + Web Platform Tests

- **WPT subset** — DOM, CSS, fetch. Цель: 60% pass к v1.0.
- **Top sites test** — на каждом релизе автоматический прогон, скриншоты, сравнение с Chromium как baseline.
- **Fuzzing** — 10 минут на PR.

#### Что значит «тестирование пораньше»

Уровни 2 и 3 — это **прямое требование** к Phase 0 (см. §16). Они существуют **для нас самих**: мы пишем `lumen-layout`, мы и тестируем его структурными ассертами, без процесс-запусков и пиксельных сравнений. Это не «отдадим тестерам потом», это «работает уже сейчас, пока движок растёт». Phase 0 не закрыт без них.

### 15.2 CI

GitHub Actions: Linux / macOS / Windows, debug + release, `cargo test` (уровни 1-3) + `cargo clippy -- -D warnings` + `cargo deny` + fuzzing 10 минут на PR. Уровень 4 (cross-browser) — отдельный ночной workflow. Уровень 5 (top sites, WPT) — релизный workflow.

### 15.3 Performance gate

`lumen-bench` (см. §16 Phase 1, §11.4) — обязательный regression-guard в CI для PR-ов, затрагивающих automation, anti-detection, tab lifecycle или сетевые слои. Baseline (`bench/baseline.json`) включает **две оси**: time и RAM.

**Time-axis baseline:** cold start ≤ 300 ms на `samples/page.html`, ≤ 500 ms на `samples/heavy.html`.

**RAM-axis baseline (расширено [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):**

| Метрика | Baseline |
|---|---|
| T0 simple page (`samples/page.html`) peak RSS | ≤ 100 MB |
| T0 heavy page (`samples/heavy.html`) peak RSS | ≤ 200 MB |
| T2 steady-state RSS per tab | ≤ 25 MB |
| T1 → T0 restore | ≤ 50 ms |
| T2 → T0 restore | ≤ 200 ms |
| T3 → T0 restore | ≤ 1500 ms |

**Правило (binding по [ADR-006](docs/decisions/ADR-006-automation-api.md), [ADR-007](docs/decisions/ADR-007-anti-detection-stack.md), [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):** PR фейлится в CI при **любом** из условий:

- > 5% регресс time-median или time-p95.
- > 5% регресс peak_rss или steady_state_rss.
- > 20% регресс любого tier-transition restore time.
- Hard budget из таблицы §11.4 превышен.

Это применяется к **default-сборке** без `--mcp` / `--bidi-port` / `--cdp-port` и без Strict / Tor профилей. Default — то, что получает каждый пользователь, и оно должно оставаться лёгким.

Если PR регрессирует:

1. Перенести стоимость за runtime-флаг (транспорт не активен → нулевая стоимость) или за `cargo` feature с `default = false`.
2. Lazy-evaluate (считать только при вызове JS API, не на каждый paint-tick).
3. Снизить интенсивность на Standard, более тяжёлый вариант оставить на Strict.
4. Перевести данные за tier'ную границу (например, dropped image cache при переходе в T2 уже даёт RAM-экономию).
5. Если ни один путь не работает — явное архитектурное обоснование в PR-body и reviewer sign-off.

CI gate (задачи 9G.3 + 9G.5 в Roadmap): `cargo run -p lumen-bench --release` + сравнение time + RAM axes + tier transitions с `bench/baseline.json` → fail при регрессе. Обновление baseline — отдельный коммит с обоснованием (задача 9G.4, процедура в `bench/UPDATE.md`).

---

## 16. Фазы разработки (реалистично)

### Фаза 0 — Прототип (3 месяца)
- ✅ Workspace, base crates.
- 🟡 HTML parser — минимум готов (см. выше).
- 🟡 CSS parser — минимум готов (см. выше).
- ✅ DOM (арена + базовые типы).
- ✅ Layout: block-flow + word-wrapping (TextMeasurer + FontMeasurer).
- 🟡 Paint: FillRect через wgpu готов; глифы — позже.
- 🟡 UI: одно окно (готово), вкладки и адресная строка — нет.
- ⬜ HTTP/1.1 + HTTPS.
- ⬜ **Automation foundation (ADR-006, §6.11)** — критично для собственного тестирования, **не отложить на потом**:
  - **`lumen-driver` крейт** с trait `BrowserSession` и `InProcessSession`. Шелл переписать как первый клиент trait-а (окно/winit/wgpu становятся одним из транспортов, не центром).
  - **Off-screen рендер** в `lumen-paint` (`Renderer::render_to_image() -> Image`) для `session.screenshot()` без winit-окна.
  - **Software rasterizer для тестов** (`tiny-skia`, opt-in под `cfg(test)`) — детерминизм пикселей между Windows/macOS/Linux CI.
  - **Тестовая пирамида уровни 2-3 включены** (§15): структурные ассерты + in-process snapshot вместо текущей ffmpeg/gdigrab-схемы. Уровень 4 (vs Edge) переезжает в ночной job.
  - **Миграция `graphic_tests/`**: каждый из 22 текущих HTML-тестов получает (а) Rust-тест в `crates/lumen-driver/tests/` со структурными ассертами по `COVERAGE.md`, (б) PNG-эталон в `graphic_tests/snapshots/`.
- ⬜ **Tab lifecycle архитектурные инварианты** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)) — **обязательно до Phase 1 finalize**, иначе ретрофит 5-10×:
  - **Invariant 1: DOM arena** — `lumen-dom` audit: убедиться что node graph на `NodeId(u32)` без `Rc<RefCell>`; добавить `bincode::serialize` для DOM snapshot; clippy lint запрещает `Rc<RefCell>` в node-модулях (трек 10B).
  - **Invariant 2: JsRuntime suspend/resume API** — расширить trait в `lumen-core::ext::JsRuntime` методами `pause()` / `unpause()` / `suspend()` / `resume()`; имплементация для `rquickjs` через `JS_WriteObject`/`JS_ReadObject` (трек 10C).
  - **Invariant 3: pure layout + paint** — audit `lumen-layout` и `lumen-paint::display_list` на отсутствие `static MUT` / `lazy_static` / `OnceCell` внутри hot path; cross-tab кэши (glyph atlas, image decode) — отдельные крейты с explicit eviction (трек 10D).
- **Цель:** открыть простую текстовую статью без стилей. Доказательство концепции, **проверяемое из Rust без запуска отдельного процесса**, с зафиксированными tier-инвариантами для будущей лёгкости вкладки.

### Фаза 1 — v0.1 «Reader» (9 месяцев от старта)
- **Базовая пригодность shell** — без этого «открыть Habr-статью» невозможно как демо:
  - **Font fallback / matcher.** Рендерер сейчас всегда `Inter Regular` — любая страница с эмодзи / CJK / `font-family: Roboto` падает в `?`-глифы. Минимум: системный font-loader (Win32 GDI / fontconfig / CoreText — без сторонних crate-ов), cascade «Inter → системный по unicode-блоку». Парсер `font-family` уже есть, не используется в paint.
  - **HiDPI / DPR-awareness.** ✅ paint-side: `Renderer` хранит `scale_factor` и делит viewport uniform на него (1 CSS px = `scale_factor` device px на 4K). ✅ Layout-side: viewport читается из `Renderer::viewport_size()` на каждый resize; `LayoutSource {document, stylesheet}` хранится в `Lumen` и переиспользуется для relayout без re-fetch/re-parse.
  - **Scroll + базовый input в shell.** Без scroll длинные страницы недоступны.
  - **Progressive / streaming rendering pipeline.** Сейчас shell блокирующий: окно создаётся **после** того, как HTML загружен, все `<link rel=stylesheet>` фетчатся **последовательно**, и только потом layout/paint. На странице с 30+ внешними CSS (Habr, любой современный сайт) пользователь смотрит в чёрный экран 5–15 секунд, после чего сразу появляется готовая страница. Это противоречит привычной модели браузера. Требуемая архитектура: (1) окно создаётся **первым**, до любых fetch-ей, пустое до прихода данных; (2) HTML fetch в фоновом потоке, chunks через channel в main thread; (3) tokenizer переделать на push-based (скармливаешь chunks — получаешь events), tree builder инкрементальный (новые узлы добавляются в существующий DOM); (4) subresources (CSS, картинки) фетчатся параллельно через thread pool / async; до прихода CSS — применяется UA stylesheet; (5) layout/paint reruns on dirty (relayout только поддерева, не всего дерева) с throttling до ~60 Гц. Касается shell + html-parser + network + layout. Большая задача, требует **архитектурного перепроектирования** main-loop shell-а и tokenizer-а. Прямо примыкает к «Network service в отдельном процессе» из той же фазы — оба про async-fetch, но streaming-парсинг и инкрементальный DOM из site isolation не следуют автоматически.
- **`Url` как структурированный тип** — `struct { scheme, host, port, path, query, fragment }`. Сейчас `Url` это тонкая обёртка над String, network ad-hoc парсит то же самое. Дедуплицировать парсинг до того, как появятся CSP / cookie jar / cross-origin checks. Несколько часов работы пока потребителей мало.
- ✅ **EventSink в network (network log).** `HttpClient::with_sink/with_tab` builder, эмит `RequestStarted` (после `parse_url`, до сокета) и `RequestCompleted` (после статус-строки, до анализа кода) — отдельная пара на каждый редирект-хоп. `StdoutEventSink` в shell печатает `→ GET <url>` / `← <status> <url>` / `✗ <url> (<reason>)`.
- ✅ **`RequestFilter` hook + `Event::RequestBlocked`.** `HttpClient::with_filter(Arc<dyn RequestFilter>)`: trait `should_block(&Url) -> Option<String>` живёт в `lumen-core::ext`, отделён от `FilterListSource` (загрузчика правил). При срабатывании эмитится `RequestBlocked { tab_id, url, reason }` ДО `RequestStarted` и до TCP — блокированный запрос не покидает клиент. Каждый redirect-hop проверяется независимо. Реализаций фильтров пока нет — место для интеграции с EasyList / собственным adblock-матчером готово.
- ✅ **`cargo bench` baseline (lumen-bench).** Бинарь, прогоняющий `decode → parse → layout → paint` на `samples/page.html` нужное число итераций и печатающий min/median/mean/p95/max на фазу + TOTAL; без сторонних deps, `LUMEN_BENCH_ITERS` env override. Регрессии при росте функциональности теперь отслеживаются (300ms cold start, <100MB RAM — точки отсчёта зафиксированы).
- ✅ **`[profile.dev.package."*"] opt-level=3`** — full optimization для зависимостей (wgpu, winit, rustls) в dev профиле, наш код остаётся на opt-level=1. wgpu в чистом debug режиме невыносим.
- CSS 2.1 + flexbox.
- Картинки.
- Вкладки, история, закладки.
- Network service в отдельном процессе.
- Storage service.
- Базовый adblock, DoH.
- **Tab session export / import** (§12.7) — простая фича, экономит много боли.
- Пакеты под Linux/macOS/Windows.
- **Browser fundamentals — критичные подсистемы, обнаруженные при аудите против Chromium / Firefox / Servo / Ladybird** (полный список с обоснованиями — в [CLAUDE.md](CLAUDE.md) → roadmap «Browser fundamentals»):
  - **HTML event loop + microtasks + rendering steps + observers** (`[P4]`) — контракт shell-а, не JS-движка. Без него ни Promise.then, ни ResizeObserver/IntersectionObserver/MutationObserver/PerformanceObserver, ни rAF не работают.
  - **Stacking contexts + правильный CSS Painting Order** (`[P1+P2]`, CSS 2.1 Appendix E) — сейчас paint в порядке DOM-обхода, z-index работает случайно. P1 — модель stacking-ов в layout; P2 — paint-side traversal.
  - **Compositor thread + property trees** (`[P2+P1]`) — TransformTree/ScrollTree/EffectTree/ClipTree на отдельном thread, off-main-thread scroll. Расширяет существующий план `compositor` крейта архитектурой. P2 — compositor pipeline + GPU; P1 — property trees от style/layout.
  - **Stacking-aware hit testing** (`[P2]`) — отдельная структура с z-index/pointer-events awareness, привязана к compositor layer tree.
  - ✅ **Quirks mode vs standards mode** (`[P1]`) — detection + application полностью реализованы 2026-05-24.
  - **Same-Origin Policy enforcement + CORS preflight** (`[P3]`) — SOP checks при fetch/postMessage/storage; OPTIONS preflight для non-simple requests.
  - **Mixed-content blocking + `<iframe sandbox>`** (`[P3]`) — HTTPS не грузит HTTP-script; sandbox flags.
  - **Preload scanner** (`[P1+P4]`) — отдельный pre-parser стартует fetch до DOM construction. Особенно важно над streaming pipeline. P1 — отдельный mode tokenizer-а; P4 — shell оркестрация.
- **Automation Phase 1 (ADR-006, §6.11):**
  - **`lumen-mcp-server` крейт** — Model Context Protocol over stdio/UNIX socket. Resources: `screenshot`, `a11y_tree`, `layout`, `console`, `network`. Tools: `click`, `type`, `scroll`, `navigate`, `wait`, `eval`. Запуск через `lumen --mcp` или `lumen --mcp-port N`. Это первый внешний транспорт — фастрастущий сегмент AI-агентов (Claude Computer Use, OpenAI Operator, Browser Use). MCP проще BiDi: JSON-RPC, маленькая спека.
  - **Native input injection** в шелле — `BrowserSession::input_event()` подаёт события в event loop тем же путём, что winit-события от ОС. Никаких `dispatchEvent` синтетических.
  - **Auto-wait внутри движка** — `wait_for(Cond::Visible/Stable/NetworkIdle/JsIdle)` на тиках layout/network/JS, не в SDK retry-loop.
  - **Per-context isolation по умолчанию** — каждая `BrowserSession` изолирована (cookies/storage/cache/viewport/UA/fingerprint).
  - **Deterministic mode** — `set_clock` / `set_rng_seed` / `freeze_fingerprint` для repeatable-тестов. Опирается на §9.5 anti-fingerprinting инфраструктуру.
  - **A11y tree first-class** — крейт `lumen-a11y` (P1) поднимается до уровня semantic locator surface; `BrowserSession::query(Role/Name/Text)` использует его, а не DOM-селекторы.
- **Tab lifecycle Phase 1** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):
  - **`TabState` enum + state machine T0-T4** (трек 10A) — состояния, transitions, per-user конфиг таймаутов.
  - **`MemoryPressureSource` trait** ✅ + три OS-impls (Win32 / Linux PSI / macOS `host_statistics64`) (трек 10H).
  - **Image decode cache LRU + viewport-gating** (трек 10E) — главный источник экономии T0: `ImageHandle` индирекция вместо прямых `DecodedImage` ссылок; decode только viewport ± 2 экрана; scroll-discard.
  - **Базовый T1 (paused)** — JS event loop pause/unpause при hide/show вкладки.
- **Цель:** ежедневный браузер для чтения статей; AI-агенты могут управлять Lumen через MCP без обёрток; **простая вкладка занимает ≤ 100 MB peak RSS**.

### Фаза 2 — v0.5 «Interactive» (18–24 месяца)
- QuickJS интеграция.
- Tier 1 Web APIs.
- Формы, базовая интерактивность.
- HTTP/2.
- GPU compositor (wgpu).
- CSS Grid.
- Site isolation.
- Профили, шифрование.
- Anti-fingerprinting.
- **Knowledge layer ядро (§12):**
  - `lumen-knowledge` крейт: FTS-индекс над историей (§12.1).
  - Аннотации и заметки (§12.2).
  - Read-later / офлайн-чтение (§12.3).
  - Поиск по содержимому открытых вкладок (§12.4).
  - Focus mode (§12.6).
- **`<meta viewport>` parsing + page zoom (Ctrl+/Ctrl-).** Без этого мобильная вёрстка всегда «как desktop», и нет ручного управления масштабом.
- **Кастомизация UI** — drag&drop панелей, темы (§12.10).
- **Browser fundamentals — Phase 2** (полный список — в [CLAUDE.md](CLAUDE.md) → roadmap «Browser fundamentals»):
  - **Shadow DOM + custom elements + `<template>` + `<slot>`** (`[P1+P4]`) — Web Components. Без них половина современных сайтов сломается. P1 — cascade + composed tree + template/slot tree-builder; P4 — JS bindings + lifecycle.
  - **Accessibility tree + platform bridges** (`[P1+P4]`) — обязательно для NVDA / Orca / VoiceOver. «Русский first-class» требует. P1 — tree construction из DOM/layout + ARIA + focus model; P4 — platform bridges (UIA / AT-SPI / NSAccessibility) + focus dispatch.
  - **Forms runtime** (`[P1+P4]`) — Constraint Validation API, submission algorithm, file picker, autofill UI поверх существующего storage. P1 — ValidityState + validation pseudo-classes + submission algorithm; P4 — native pickers + autofill popup + validation tooltip.
  - ✅ **`<picture>` / `srcset` / `sizes` + `loading="lazy"`** (`[P1+P2]`) — P1 завершён: srcset, sizes, picture-picker, IntersectionObserver event source для lazy (rootMargin). P2 — image GPU upload.
  - **IME composition events** (`[P4]`) — без них японский / китайский / корейский ввод сломан.
  - **Connection pooling + keep-alive + Brotli + Range requests** (`[P3]`, ✅ keep-alive + Brotli + single-range; ⬜ multi-range / suffix / If-Range) — без keep-alive реальный сайт = 50× TCP handshakes.
  - **Find in page (Ctrl+F)** (`[P4]`).
  - **DevTools / Inspector минимум через CDP** (`[P4]`) — DOM tree + computed styles + network log. Без этого debug собственного движка невозможен.
  - **`mix-blend-mode` / `backdrop-filter` / `isolation`** (`[P1+P2]`) — нужны isolation groups в compositor pipeline. P1 — parsing + stacking model; P2 — paint pipeline + isolation groups.
- **Automation Phase 2 (ADR-006, §6.11):**
  - **`lumen-bidi-server` крейт** — WebDriver BiDi subset over WebSocket. Цель: `playwright.connect('ws://localhost:9222/session')` работает из коробки. Запуск через `lumen --bidi-port N`.
  - **Ship BiDi-gaps как built-in** — то, чего нет в W3C Working Draft (см. Playwright #32577, Cypress #30447): full response body access, `resourceType`, locale/timezone/offline emulation, per-context UA + extra headers, viewport-before-popup, per-context preload scripts, full download lifecycle, cookie change events, per-origin storage clear, дешёвая network interception. Документировать gap-mapping в `subsystems/lumen-bidi-server.md`.
  - **Espresso/Computer-use bridge для тестировщиков** — заранее закладывается accessibility-tree query API через MCP, аналогичный Playwright `getByRole`, чтобы тесты не зависели от CSS-классов и переживали DOM-рефакторы.
- **Tab lifecycle Phase 2** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):
  - **T2 (JS heap snapshot)** — async-save в SQLite при T1→T2 (трек 10I); async-load с indeterminate UI hint при > 100ms; zstd compression; cap 5 MB/tab disk.
  - **T3 (full hibernation)** — DOM serialization через `bincode + zstd` в SQLite (трек 10J); в RAM остаётся только `TabMetadata` (URL, title, scroll, favicon) <200 KB/tab.
  - **GPU layer LRU + texture recycling** (трек 10F) — `wgpu::Texture` pool для off-viewport stacking contexts.
  - **Glyph atlas LRU eviction** (трек 10G).
  - **UI affordance** (трек 10K) — иконка "Z" / fade-opacity на спящих вкладках в tab strip, tooltip с tier-info, loading-spinner при restore > 200ms.
  - **JS heap GC tuning per tier** (трек 10L) — мягкий GC для активной, агрессивный для idle.
- **Цель:** публичная альфа, форумы и простые SPA, в Lumen начинают **жить** долго; Playwright/Selenium/Cypress тесты сторонних команд работают на Lumen; **50 открытых вкладок ≤ 600 MB total RAM**.

### Фаза 3 — v1.0 (36–48 месяцев)
- Переход на V8 (`rusty_v8`).
- Tier 2 Web APIs.
- IndexedDB, Canvas 2D.
- HTTP/3.
- Service Workers.
- WebFonts (WOFF2).
- Расширения (свой минимальный формат).
- WPT pass rate ≥ 60%.
- **Опциональный AI-модуль (§12.5):** `lumen-ai` крейт за feature-флагом. Семантический поиск, суммаризация, RAG над собственной историей. Bundle без AI остаётся basic-вариантом.
- **Семантические закладки (§12.8)** — опционально, требует AI.
- **Browser fundamentals — Phase 3+** (полный список — в [CLAUDE.md](CLAUDE.md) → roadmap «Browser fundamentals»):
  - **WebSockets (RFC 6455) + Server-Sent Events + Fetch API runtime с AbortController** (`[P3]`).
  - **HTTP auth (Basic + Digest)** (`[P3]`, готово) — `HttpClient::with_credentials` + RFC 7617/7616 в `lumen-network::auth`. Negotiate/NTLM + client certificates (mTLS) — отложены.
  - **OCSP stapling + CT log enforcement + invalid cert UI** (`[P3]`).
  - **Safe Browsing equivalent** (`[P3]`, готово) — `SafeBrowsingList` (SQLite) + `SafeBrowsingFilter` поверх `RequestFilter`-точки; полные SHA-256 + 20 канонических вариантов на URL; без облачного API.
  - **Back/forward cache (bfcache)** (`[P4]`).
  - **Navigation API + History API runtime** (`[P4]`).
  - **Web Animations API runtime** (`[P1+P2+P4]`) — compositor-driven для transform/opacity. P1 — value interpolation в момент t; P2 — compositor offload; P4 — animation timeline scheduling.
  - **`<contenteditable>` + Input Events Level 2 + Selection / Range API** (`[P1+P4]`) — P1 — DOM mutations + Selection/Range типы + `beforeinput`/`input` event firing; P4 — input dispatch (keyboard / IME / drag-drop / paste) + undo stack.
  - **Service Worker runtime** (`[P3+P4]`) — fetch interception / push / background sync. P3 — fetch interception API + push delivery + bg sync queue; P4 — отдельный JS worker context + lifecycle.
  - **Spell check** (`[P3+P4]`) через Hunspell-словари — русский словарь обязателен. P3 — словарь loader / Hunspell-формат parser / storage; P4 — squiggly render + context menu + OS API integration.
  - **Variable fonts axes runtime** (`[P2]`) — `font-variation-settings`.
  - **Color management + Display P3 / Rec2020 / ICC** (`[P2]`).
  - **Print pipeline runtime** (`[P1+P2+P4]`) — pagination algorithm над уже parsed `@page` и break-* properties, PDF generation. P1 — pagination algorithm; P2 — PDF rendering из display list; P4 — print preview UI.
  - **GC integration JS ↔ DOM** (`[P1+P4]`) — cycle collector между Rust DOM и JS engine. Архитектурная задача при интеграции QuickJS / V8. P1 — DOM wrapper hooks; P4 — JS engine integration + cycle collector algorithm.
  - **Permission prompt UI + Download UI** (`[P4]`) поверх существующего permissions/downloads storage.
  - **GPU process / sandbox** (`[P4]`) — seccomp / AppContainer / App Sandbox, расширение site isolation.
- **Automation Phase 3 (опционально, по запросу):**
  - **`lumen-cdp-shim` крейт** — Chrome DevTools Protocol subset как **thin adapter** поверх `BrowserSession`. Triggered only by real named demand from a legacy Puppeteer-using project. До этого CDP-кода в Lumen нет (см. ADR-006 «Graduation triggers»).
- **Цель:** стабильный релиз.

### Фаза 4 — После 1.0
- Подмножество WebGL (по запросам).
- Mobile (Android через NDK; iOS — упрётся в Apple-policy).
- **Sync через E2E (§12.11)** — self-host или P2P. Mobile-клиент критичен для real use-case.
- **Граф знаний (§12.9)** — визуализация коллекции.
- Локализация UI.

---

## 17. Команда и ресурсы

| Фаза | Состав | Длительность |
|---|---|---|
| 0 — прототип | 2 senior Rust | 3 мес |
| 1 — v0.1 | 3–4 (Rust, систем, UX) | 9 мес |
| 2 — v0.5 | 5–7 (+ JS-эксперт, security) | 12–18 мес |
| 3 — v1.0 | 8–12 | 18–24 мес |

Бюджетная оценка: **минимум 4–5 миллионов USD до v1.0** (если коммерчески), или 4–5 лет с маленькой full-time командой энтузиастов.

---

## 18. Риски и митигация

| Риск | Митигация |
|---|---|
| Веб слишком велик, не успеваем за стандартами | Фокус на читаемый веб, явный scope, отказ от экзотики |
| JS-биндинги хрупкие, текут CVE | Изоляция unsafe, fuzzing, ревью каждой биндинг-функции |
| Сайты ломаются (думают, что мы IE) | UA fixed на актуальный Chrome для совместимости |
| Compositor нестабильный на разных GPU | wgpu абстрагирует, тестируем на 3 GPU min (NV/AMD/Intel) |
| Memory safety не спасает от логических уязвимостей | Sandbox, site isolation, audit |
| Apple запрещает свои движки на iOS | iOS откладываем; либо тонкая обёртка над WKWebView под iOS как исключение |
| Выгорание | Жёсткий scope, чёткие версии, регулярные релизы |
| Supply chain (crates.io) | `cargo-vet`, `cargo-deny`, минимизируем зависимости |
| Accessibility tree (MSAA/UIA/AT-SPI/NSAccessibility) — сотни тысяч строк, без AX браузер не работает со screen reader-ами; в США/EU юридическое требование для коммерческого продукта | AX откладываем до Phase 4. До тех пор Lumen честно объявляется как не подходящий для слепых пользователей. Архитектурный задел в DOM (semantic tree уже есть) минимален — основная работа OS-bindings |
| DRM (Widevine, FairPlay, PlayReady) — Widevine лицензируется только Google, не-Chromium форкам почти не выдаётся (Brave получил после многолетнего процесса; LibreWolf и Tor Browser живут без него) — значит Netflix / Spotify Web / большинство streaming сервисов недоступны | Принимаем как явный non-goal v1.0: «Lumen не воспроизводит DRM-контент». В Phase 4 можно попробовать процесс лицензирования, но не блокируем релиз. AV1 / H.264 декодеры — отдельная задача (FFmpeg как 6-й exception или dav1d / openh264) |
| Печать (`@media print`, OS print spooler — CUPS / CoreGraphics / Windows spooler) — требует отдельного layout path и интеграции с тремя OS | Откладываем до Phase 3. Минимум — экспорт в PDF через свой layout-pipeline (свой PDF writer — реалистичнее, чем OS-биндинги). PDF-генерация — единый код-путь для всех OS |
| Шрифты на реальных страницах — нет fallback, рендерер всегда Inter; CJK / эмодзи / явные `font-family` ломаются | Font matcher в Phase 1 (см. секцию «Базовая пригодность shell»). Без этого Phase 1 как демо невозможна |

---

## 19. Лицензия

- **MPL-2.0** — позволяет связывание со внешним кодом, требует open-source модифицированных файлов. Совместимо с экосистемой Servo/Firefox.
- DCO вместо CLA.
- Публичный roadmap, RFC-процесс.

---

## 20. Первые конкретные шаги

1. `cargo new --bin lumen` + создать workspace с пустыми crates.
2. `engine/html-parser` — свой токенизатор (FSM по HTML5 spec), затем tree construction. Прогнать `html5lib-tests` (тесты — внешние данные, не код).
3. `engine/css-parser` — свой токенизатор + parser + selectors.
4. `engine/dom` — арена, NodeId, базовые API.
5. `engine/layout` — свой block + inline.
6. `engine/paint` — свой CPU-растеризатор; нарисовать первый бокс в окне.
7. `shell` — окно winit + egui, рендер картинки от движка.
8. **Веха «hello world»:** открыть страницу `<html><body><h1>Hello</h1></body></html>` локально, увидеть текст.
9. **Веха «Внешняя страница»:** открыть удалённую текстовую статью по HTTP, прокрутить, перейти по ссылке.
10. После этого — `network` отдельным процессом, IPC.

---

## 21. Чего я НЕ обещаю

- Что v1.0 будет «как Chrome». Не будет. Будет браузер, в котором работает 80% сайтов и который вы понимаете до последней строки.
- Что это коммерчески выгодно. Скорее всего, нет — это исследовательский / идеологический проект.
- Что Servo/Ladybird не обгонят. Возможно, обгонят. Тогда имеет смысл влить силы туда.

---

## 22. Документация для пользователя

В §8.4 зафиксировано: **welcome-screens, in-app туториалов и бейджей не делаем** — это противоречит принципу №5 «Стабильный UI». Пользователь, открывший Lumen впервые, видит обычное окно, не модалку. Но документация всё равно нужна — иначе knowledge layer (§12) и приватностные пресеты (§9.5) останутся незамеченными. Решение — **документация снаружи браузера**, в репо и на landing page.

### 22.1 Структура

```
docs/
├── tutorial/                  — для пользователя
│   ├── 01-start.md            — первый запуск, открыть страницу
│   ├── 02-omnibox.md          — омнибокс с префиксами @history / @tabs / @notes
│   ├── 03-workspaces.md       — разделение work / personal / project
│   ├── 04-network-log.md      — что показывает Ctrl+Shift+N
│   ├── 05-knowledge.md        — поиск по истории, аннотации, read-later
│   ├── 06-privacy.md          — Standard / Strict / Tor-mode пресеты
│   ├── 07-keybinds.md         — горячие клавиши списком
│   └── images/                — PNG скриншоты, генерируются автоматически
├── architecture/              — для разработчика / контрибьютора (CLAUDE.md, plan)
└── decisions/                 — будущий ADR-каталог (см. roadmap)
```

### 22.2 Story-структура туториала

По принципу «проблема → решение» (а не «список фич»):

| Раздел | Проблема, которую видит пользователь | Решение в Lumen |
|---|---|---|
| 01-start | «Я открыл — что дальше?» | Окно открывает URL / файл, F5 reload, Esc выход |
| 02-omnibox | «У меня 80 вкладок, я ничего не найду» | `@tabs <query>` — поиск по содержимому, не только URL |
| 03-workspaces | «Личное и работа смешалось» | Workspaces с per-workspace cookies |
| 04-network-log | «Что вообще делает мой браузер?» | Network log всегда видим, каждый исходящий байт логируется |
| 05-knowledge | «Я читал что-то про X две недели назад» | FTS по истории, заметки, офлайн-копии |
| 06-privacy | «Tor-mode? Strict? Что выбрать?» | Три пресета, явные trade-off-ы |

**Первые 1-2 раздела** должны показывать **проблему, которую решает Lumen**, не features. Это принцип маркетинговой story-структуры, применённый к operational docs.

### 22.3 Скриншоты — генерируются, не рисуются

- `lumen --screenshot <output.png>` CLI flag — рендерит первый кадр в PNG и завершается. Шаг `winit` → `wgpu::read_buffer` → PNG-encoder (свой, через `lumen-image`). **Двойное назначение:**
  - **Туториал:** скрипт `tools/make-tutorial-images.rs` запускает Lumen с разными `samples/tutorial-XX-*.html`, снимает PNG, накладывает подписи (стрелки, рамки, выноски) через свой image-композитор поверх `lumen-image`. При изменении UI — `make tutorial-images` регенерирует всё.
  - **CI visual regression:** дополнение к существующим snapshot-тестам display-list (которые тестируют paint-команды, а не пиксели). Pixel-snapshot-ы для критичных страниц — golden PNG, при изменении сверяются pixel-diff-ом. Бюджет PNG-разницы (например, ±2 значения на канал) даёт устойчивость к минорным расхождениям GPU-драйверов.

### 22.4 Принципы текстов

- **Что произошло** — одной фразой.
- **Почему** — если можем сказать.
- **Что делать** — конкретно. Не «попробуйте позже», а «проверьте имя сайта».
- **Без жаргона** в первом абзаце. Технические детали — раскрываемы (`<details>`), не в лицо.

Это касается и туториала, и **user-facing error-сообщений** (тоже к §9.8): error.rs возвращает строки на английском для разработчика, но в UI пользователь видит локализованный wrapper с тем же контентом.

### 22.5 Локализация

- Основной язык — **русский** (принцип №7 «Русский язык — first-class»).
- **Английская версия — параллельно**, не вторым приоритетом: проект публичный, contributors могут не знать русского.
- Хранилище: `docs/tutorial/ru/01-start.md` + `docs/tutorial/en/01-start.md`. Не мухлевать через `i18next`-style placeholder-ы — markdown файл проще ревьюить.

### 22.6 Откуда пользователь попадёт в туториал

- README репо — раздел «Getting started» ссылается на `docs/tutorial/ru/01-start.md`.
- Landing page (когда появится): `lumen-browser.ru/tutorial`.
- **Не из браузера автоматически.** В Settings → Help может быть пункт «Open documentation» — но клик пользователя, не модалка на старте.

### 22.7 Фазы

- **Phase 0:** `lumen --screenshot` CLI + 1-2 раздела туториала (`01-start`, `04-network-log`) — то, что уже можно показать.
- **Phase 1:** туториал доводится до `02-omnibox` / `03-workspaces` по мере появления соответствующих UI.
- **Phase 2:** разделы про knowledge layer (§12.1-12.4) и read-later (§12.3).
- **Phase 3+:** AI / семантические закладки (§12.5, §12.8).
