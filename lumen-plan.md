# Lumen — браузер на Rust с собственным движком

> **Lumen** (лат. *свет*, единица светового потока) — приватный, лёгкий, прозрачный браузер. Имя отражает философию проекта: показывать пользователю всё, что происходит, и не быть тяжелее, чем нужно.

## 🔄 В работе сейчас

Задачи, взятые в работу параллельными сессиями. **Не дублировать.** Подробнее о протоколе — в `CLAUDE.md`, раздел «Координация параллельных сессий».

Над проектом параллельно работают **3 программиста** (P1–P3). Раскрой задач по программистам и доменные зоны — в `CLAUDE.md`, раздел «Распределение задач между программистами». Если в сессии тебе сказали «ты программист N» — твои задачи помечены `[PN]` в разделе «Roadmap — приоритизация задач» этого файла.

Формат строки резервации: `- 🔄 <имя задачи> [PN] — <имя ветки> — <YYYY-MM-DD>`.

- 🔄 Paint order consumer (P2 2A renderer-side) [P2] — paint-order-consumer — 2026-05-15
- 🔄 CSS image-rendering parsing/storage [P1] — css-image-rendering — 2026-05-15
- 🔄 transition shorthand parsing (P1 3A finish) [P1] — transition-shorthand — 2026-05-15

## Статус реализации

**Текущая фаза:** Phase 0 (прототип). Этот блок обновляется при каждом коммите, реализующем пункт плана. Условные обозначения: ✅ готово · 🟡 в работе · ⬜ запланировано.

### Инфраструктура
- ✅ Cargo workspace, 10 крейтов
- ✅ `rust-toolchain.toml` (stable + rustfmt + clippy)
- ✅ `.gitattributes` (LF в репо, кросс-платформенные line endings)
- ✅ Ветка `main`, локальные коммиты, без remote

### Крейты
- ✅ `lumen-core` — типы и trait-ы: `Error`, **структурированный `Url` (scheme/host/port/path/query/fragment + serialized cache, methods host_ascii/effective_port/path_and_query/resolve)**, `Event`, `Capability`, `Module`, геометрия (`Rect`, `Point`, `Size`), `NetworkTransport`, `StorageBackend`, `SearchProvider`, `FilterListSource` (загрузчик rules text), **`RequestFilter`** (per-URL `should_block(&Url) -> Option<String>`), `EncodingDetector`, **`EventSink`** (`emit(&Event)`, приёмник `Event::Request*` из подсистем), **`DnsResolver`** (`resolve(hostname, port) -> Result<Vec<SocketAddr>>`; trait-точка для system / cached / DoH / DoT резолверов), **`HstsEnforcement`** (`is_https_only(host, now_unix) -> bool` + `record_sts(host, max_age, include_subdomains, preload, now_unix)`; без `Result`, fail-open; реализация в `lumen-storage::hsts::HstsStore`, потребитель — `lumen-network::HttpClient::with_hsts(...)` для RFC 6797 http→https upgrade и persist `Strict-Transport-Security`), **`HttpCredentialProvider`** (`credentials(&HttpAuthChallenge { origin, realm, scheme: HttpAuthScheme::{Basic|Digest} }) -> Option<HttpCredentials { username, password }>`; HTTP auth по RFC 7617 / RFC 7616, потребитель — `lumen-network::HttpClient::with_credentials(...)`), **`JsRuntime`** (eval / set_global / get_global / call_function через `JsValue` JSON-совместимые типы; `NullJsRuntime` stub возвращает `JsError::NotImplemented`; первая реальная реализация — QuickJS / rusty_v8). **Sprint 0 P3 trait-anchors** (interface готов, Null-stub-ы — «не поддерживается»; интегрируются через provisional-крейты по §5): **`UnicodeProvider`** (UAX #14 line-break / UAX #29 segmentation / UAX #9 bidi — под `icu4x.segmenter` + `icu4x.linebreak`), **`IdnaProvider`** (UTS #46 to_ascii/to_unicode — под `idna`-crate), **`PublicSuffixList`** (eTLD / eTLD+1 / is_public_suffix для cookie matching + Safe Browsing host-suffix — под `publicsuffix`-crate, P3 п.2B), **`ContentDecoder`** (HTTP `Content-Encoding` encoding+decode; есть `UnsupportedContentDecoder { encoding }` stub возвращает `Error::Other` — под расширения `brotli-decompressor` / `ruzstd` / `zstd-safe`, P3 п.1A), **`FontFormat`** (format_name/can_decode/decode_to_sfnt — под `woff2` для WebFonts), **`SpellChecker`** (check/suggest/locale — под `hunspell-rs` / `spellbook`; `NullSpellChecker::check` всегда true, чтобы UI не подчёркивал всё), **`HyphenationProvider`** (hyphenate/locales — под `hyphenation` с TeX-словарями). Модули **`punycode`** (RFC 3492 encode) + **`idn`** (`domain_to_ascii`) для IDN-доменов
- ✅ `lumen-dom` — арена + `NodeId` + `Document/Node/NodeData`, API: create/append/detach/Display, **`DocumentMode` enum + `Document.mode/set_mode`** (HTML5 §13.2.6.2 — выставляется tree builder-ом по DOCTYPE, см. `quirks_mode`), **`Document.target_id` + `target()`/`set_target(Option<S>)`** (CSS Selectors L4 §9.6 + HTML LS §7.10.6 — id из URL fragment для `:target` matcher-а; setter фильтрует empty string в `None`; выставляется shell-интеграцией P3 при навигации, не tree builder-ом), 30 тестов (включая кириллицу)
- 🟡 `lumen-shell` — точка входа: три режима (пустое окно / файл / URL). Внешний CSS через `<link rel=stylesheet>`: загружается с диска (относительно HTML-файла) или по сети (относительно базового URL). Bundled Inter-Regular.ttf через `include_bytes!`. **HTML event loop framework + integration в winit-loop + task source priorities + requestIdleCallback** в `lumen-shell::runtime`/`Lumen` — per-source TaskQueue (`[VecDeque; 7]`, обход в `TaskSource::PRIORITY_ORDER`: `UserInteraction > DomManipulation > HistoryTraversal > Networking > Timer > Rendering > IdleTask`) + MicrotaskQueue (drain-all) + EventLoop::step, rAF с cancel, idle-callback-и с `IdleDeadline {time_remaining, did_timeout}` + опциональным `timeout_ms` (абсолютный override caller-budget), observer registries (Resize/Intersection/Mutation), reentrancy через `Rc<RefCell>` + `EventLoopHandle::clone`; Lumen дёргает `about_to_wait → step()×N` (cap 256), `Resized → deliver_observer_records(Resize)`, `RedrawRequested → run_rendering_step(timestamp_ms)` перед render. 32 unit-теста runtime. **Find in page (Ctrl+F)** в модуле `lumen-shell::find`: поиск по `DrawText`-командам display list через `TextMeasurer` (case-insensitive, Unicode-aware, non-overlapping), `FindState { open, query, active }` с next/prev cycling, `build_with_overlay` вставляет FillRect-подсветки перед своими `DrawText` (active=оранжевый, inactive=жёлтый) + UI bar (Найти / input / counter) в правом верхнем углу. Ctrl+F открывает, Esc закрывает, Enter/F3=next, Shift+Enter/Shift+F3=prev, Backspace стирает, остальные символы из `KeyEvent.text` идут в query. Reload сбрасывает find (display list другой). 24 unit-теста find. Phase 0 ограничения: reload через queue_task отложен (требует Rc<RefCell<Lumen>>-рефакторинга под JS engine), integration `run_idle_callbacks` в Lumen-loop отложена, scroll-to-match для find отложен до реализации scroll.
- 🟡 `lumen-html-parser` — минимальный токенизатор (Data/Tag/Attribute/Comment, **расширенный набор ~250 named entities** через сортированную const-таблицу + numeric, **RAWTEXT для `<script>`/`<style>`**, **RCDATA для `<title>`/`<textarea>`**, **DOCTYPE с PUBLIC/SYSTEM** — public_id/system_id как `Option<String>` чтобы различать missing/empty) + lenient tree builder. **Модуль `quirks_mode::detect_document_mode` (HTML5 §13.2.5.1)** — exact-match public/system IDs + ~55 prefix-правил + HTML 4.01 Frameset/Transitional + XHTML 1.0 правила; tree_builder применяет detection при первом DOCTYPE-токене, при отсутствии DOCTYPE — Quirks (§13.2.6.4.1). **Модуль `srcset` (HTML5 §4.8.4.3.5 + §4.8.4.3.7 + §4.8.4.4)** — `parse_srcset` / `pick_best_for_density(dpr)` + `parse_sizes` / `evaluate_sizes(viewport)` / `pick_best_for_width(source_size_px, dpr)` для `<img srcset sizes>` / `<source srcset>`; density (`Nx`) и width (`Nw`) descriptors, sizes-атрибут с media-condition (min-/max-width|height + orientation + **prefers-color-scheme**, AND-list, **ведущий `not` для инверсии clause** — Media Queries L4 §3.2, lenient-вариант «not ко всей AND-цепи»; **L4 nested-формы `(not <cond>)` и `((<cond>))`** внутри clause-скобок — strict per spec `<media-not> = not <media-in-parens>` через `MediaClause::Nested(Box<MediaCondition>)` + paren-aware top-level split-by-`and`); SizesViewport `{width_px, height_px, root_font_size_px, prefers_dark}` + SizeLength (Px/Em/Rem/Vh/Vw/Vmin/Vmax/Percent); `parse_media_condition` экспортирован публично для re-use в picture-picker-е. **Модуль `picture` (HTML5 §4.8.4.4)** — `pick_picture_source(doc, picture_node, &PictureParams)` walks `<source>` детей `<picture>` в source-order с фильтрами по `type` (case-insensitive lookup в `supported_types: Option<&[&str]>`; `None` отключает фильтр; пустой `type=""` = match-everything) и `media` (тот же media-AST что у sizes), pick через srcset+sizes (width-picker для Nw, density для Nx), fallback на первый `<img>` ребёнок; `pick_img_source` — отдельный entry для одиночного `<img>` (srcset+sizes → src; пустой `src` → None). `PickedSource { url, intrinsic_width: Option<u32>, intrinsic_height: Option<u32> }` — author-объявленные dimensions из `<source|img width|height>` для CLS-protection (HTML5 §10 «mapped attributes»; `parse_dim_attr` — leading digits, отрицательные/percent отбрасываются). **Модуль `preload_scanner` (HTML LS §13.2.6.4.7)** — `scan_preload_hints(&str) -> Vec<PreloadHint>` бежит поверх Tokenizer-а без построения DOM и эмитит `Stylesheet`/`Script`/`Image`/`SourceSet`/`Preload`/`Preconnect` hints в source-order; `rel` multi-token, RAWTEXT внутри `<script>`/`<style>` корректно не парсится как теги. **P1 п.3B — push-tokenizer + incremental tree builder**: `PushTokenizer::feed(&str)` / `end() -> Vec<Token>` — обёртка над тем же pull-Tokenizer-ом, поверх owned-`String`-буфера + эвристики `find_safe_split` (учитывает `<!--…-->`, `<!DOCTYPE…>`, `<tag…>`, `&entity;` в data state и в RCDATA). Pull-Tokenizer изменён: при EOF в text-only loop восстанавливает `text_only`-поле (раньше `.take()` терял state в push-режиме). Публичные `Tokenizer::with_state(input, text_only)` + `pos()` + `text_only_state()` для возобновления между chunk-ами. `IncrementalTreeBuilder { feed(&str), finish() -> Document }` — push-вариант `parse()`-функции, держит `Document`+stack+seen_doctype между вызовами и применяет токены через общий с `parse()` `apply_token`-helper. **Инвариант: pull и push дают побайтово равный DOM** — обеспечивается text-node coalescing в `apply_token` (если последний ребёнок — text, дописываем; pull выдаёт цельный Text-токен, для него no-op). UTF-8: caller отвечает за code-point boundary в chunk-ах. Разблокирует P3 п.4B (streaming pipeline). 335 тестов. Отложено: CDATA, полный набор named entities (~2125 имён в spec), legacy без `;`, insertion modes, application Quirks-mode переключений в layout/cascade, `calc()`/`min()`/`max()` в length-значениях sizes, `loading="lazy"` через IntersectionObserver.
- 🟡 `lumen-css-parser` — расширенные селекторы: simple (type/class/id/universal/attribute/pseudo), compound (`p.foo#bar`), complex с combinator-ами (` `, `>`, `+`, `~`); attribute-операторы `=`, `~=`, `|=`, `^=`, `$=`, `*=` с **ASCII case-insensitive флагом `[a=v i]`** (CSS L4 §6.3.6); structural pseudo-classes (`:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`, `:first-of-type`, `:last-of-type`, `:only-of-type`); функциональные pseudo (`:nth-child(an+b [of <selector-list>])`, `:nth-last-child(an+b [of <selector-list>])` — `of` clause из CSS Selectors L4 §6.6.5.1 фильтрует sibling-pool до nth-индексации; `:nth-of-type`, `:nth-last-of-type` с ключевыми словами `odd`/`even`; **CSS Selectors L4 `:not(selector-list)`** — §5.4, selector-list с combinator-ами и nested `:not(:not(...))`, specificity = max-of-list; **CSS4 `:is(selector-list)` и `:where(selector-list)`** — selector-list внутри, specificity = max-of-list для :is, 0 для :where); **form-state pseudo (CSS Selectors L4 §14.2/§15.4/§15.5, HTML5 §4.10.3/§4.10.19/§4.16.4)** `:required`/`:optional`/`:read-only`/`:read-write`/`:disabled`/`:enabled` — pure attribute-based matcher-ы в layout (fieldset disabled-наследование с исключением первого `<legend>`-ребёнка, option наследует disabled от optgroup, read-only по умолчанию для не-form элементов per spec, contenteditable inheritance для read-write); **UI-state pseudo (CSS Selectors L4 §10.1/§10.2/§10.4, HTML5 §4.16.3)** `:checked`/`:indeterminate`/`:default` — pure DOM-based matcher-ы в layout (checkbox/radio через `checked`-атрибут, option через `selected`; radio-группа indeterminate через scope ближайшего `<form>`-предка с проверкой single-checked по `name`; default-submit для первой submit-кнопки внутри формы; checkbox indeterminate всегда false без runtime form-state); **`:lang(<language-tag>#)`** (CSS Selectors L4 §11) — функциональный с comma-list BCP 47 tags, matcher по RFC 4647 basic filtering против content-language (`lang`/`xml:lang` атрибут + наследование от ancestor-ов; `lang=""` — «явно неизвестен», не наследует); **`:dir(ltr|rtl)`** (CSS Selectors L4 §13.2) — functional pseudo с `DirArg::Ltr|Rtl` enum-аргументом, matcher walking up parents по `dir`-атрибуту с HTML5 §3.2.6.1 fallback на `ltr`; `dir="auto"` в Phase 0 без UAX #9 first-strong scan трактуется как `ltr` (real auto-direction отложен до bidi-движка); **link pseudo (CSS Selectors L4 §6.2)** `:link`/`:visited`/`:any-link` — pure DOM-based matchers в layout: `:any-link` и `:link` ↔ `<a>`/`<area>`/`<link>` с `href`-атрибутом (HTML5 §4.6.1 hyperlink), `:visited` всегда `false` (Phase 0 без history-runtime, privacy-safe default); **`:scope`** (CSS Selectors L4 §4.2) — root of selector matching context; в author-CSS без querySelector-runtime matches document root element (эквивалент `:root`); **`:target`** (CSS Selectors L4 §9.6) — pure DOM-based matcher: element с `id` равным `Document::target()` (URL fragment без `#`, case-sensitive per HTML LS §3.2.6); functional-формы `:target(x)` отбрасываются в `Unsupported`. Shell-интеграция (set target_id из URL fragment) — отдельная P3-задача, до неё matcher всегда `false` (privacy-safe default); **`:target-within`** (CSS Selectors L4 §9.7) — element сам `:target` ИЛИ has-descendant с `:target`; реализация — `matches_target_within` short-circuit при `Document::target() == None`, иначе свой `id` + `any_descendant` обход поддерева; без зависимости от matcher-а `:has`; **`:defined`** (CSS Selectors L4 §6.4.1, HTML LS §4.13.5) — pure DOM-based matcher: built-in HTML/SVG/MathML элементы и зарегистрированные custom elements. В Phase 0 без `CustomElementRegistry` matcher использует аппроксимацию по HTML LS §4.13.2: имя custom-element-а обязано содержать ASCII `-`, поэтому `defined = !name.contains('-')`. Поддерживает FOUC-protection idiom `:not(:defined) { display: none }`; interactive (`:hover` и т.д.) парсятся, не матчат; pseudo-elements `::name` (парсятся, не матчат). Specificity по CSS Selectors Level 3+4. **`!important` флаг в `Declaration`** (CSS Cascade L4 §8.1). **Custom property declarations (`--name: value`)**. **`@property` rules** для регистрации custom properties с syntax/inherits/initial-value. **`@media` rules** (Media Queries L4): MediaQuery с OR-list `MediaQueryClause { negated, conditions }`, MediaCondition (MediaType / Feature / Unsupported), MediaFeature (min/max-width/-height, orientation, prefers-color-scheme); ведущие `not` (инверсия clause) / `only` (L3 backcompat no-op) распознаются с whitespace-/`(`-границей (чтобы `notepad` не разваливался). `Unsupported` под `not` остаётся unknown = false (spec §3.2). 119 тестов (+5). Отложено: namespace prefix, типизированные значения деклараций других видов (length / color / calc — типы хранятся в layout)
- 🟡 `lumen-layout` — block-flow + **inline-flow** + **replaced (`<img>`)** с specificity-based style cascade, **CSS-wide keywords (inherit / initial / unset / revert по CSS Cascade L4 §7)** и line wrapping: compound и complex selectors (combinators, attribute, structural и функциональные pseudo, `:not`), наследование (color, font-size, line-height, text-align, text-decoration), color (полный CSS3 набор из 147 named colors + rebeccapurple из CSS4 + transparent + hex 3/4/6/8 digit + rgb/rgba/hsl/hsla с modern syntax), display (block/inline/none), margin/padding (включая shorthand), text-align (left/center/right), text-decoration (underline / overline / line-through, можно комбинировать, `none` сбрасывает; + L3 longhands `text-decoration-style` (solid/double/dotted/dashed/wavy) и `text-decoration-thickness` (auto/from-font/`<length>`/`<percentage>`) — Phase 0 parsing+storage, реальный рендеринг в P2), **width / height (px)**, **border (width/style/color, все shorthands, box model)**, **box-sizing (content-box / border-box)**, **CSS Variables L1 (`--name` + `var()`)** — `ComputedStyle.custom_props: HashMap`, inherited по спеке; отдельный custom-pass между font-size pre-pass и main-pass применяет все `--name: value` декларации с уважением к specificity / !important / source order; в main-pass `var(--name [, fallback])` разворачивается рекурсивно в value перед стандартным парсингом свойства (depth limit 32, циклы дают «invalid at computed value time» — declaration ignored), **CSS math-функции (Values L4 §10, §10.6, §10.7-10.9)** — `Length::Calc(Box<CalcNode>)` с базовыми (Add/Sub/Mul/Div/Min/Max/Clamp) и `Func(MathFn, args)` для 17 научных функций (sin/cos/tan/asin/acos/atan/atan2/pow/sqrt/exp/log/hypot/abs/sign/mod/rem/round); recursive-descent парсер с приоритетами `*//` > `+-`, скобки, унарный минус, nested function calls; angle-units (deg/rad/turn/grad) лексер конвертирует в радианы; работает с любыми поддерживаемыми единицами + unitless для умножения; поверх var()-substitution (`width: min(var(--w), 50px)`, `width: calc(pow(2, 5) * 1px)`). Length-units: px, em, rem, % (em/rem/% для font-size и line-height; % в margin/padding пока игнорируется до containing-block). `TextMeasurer` trait + `layout_measured()` для word-wrap по реальным шрифтовым метрикам. `InlineRun` объединяет текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`, `<strong>`, и т.д.) в один поток строк с per-сегментными стилями; каждый `InlineFrag` хранит свою ширину для align_lines и подрисовки декорации. `serialize_layout_tree` + golden snapshot-тесты (`UPDATE_SNAPSHOTS=1` для регенерации). **Sprint 0 контракты P1**: модули `stacking` (`StackingContextId`/`PaintPhase`/`PaintOrder`/`StackingTree`), `property_trees` (`TransformTree`/`ScrollTree`/`EffectTree`/`ClipTree` + `Mat4` + `PropertyTreeNodeId`), `animation` (`AnimValue` + `AnimationInterpolator` trait + `NoopInterpolator` step-half stub + `LinearInterpolator` + **`AnimValue::TransformList(Vec<TransformFn>)` с matched-pair + 2D matrix decompose fallback** по CSS Transforms L2 §15 + **`AnimValue::FilterList(Vec<FilterFn>)` с matched-pair lerp + lacuna-padding по CSS Filter Effects L1 §6**) — interface-first типы для P2 compositor / painting order и для P1 п.2A/2B/3A. **P1 п.2B — Property trees построение**: `PropertyTrees::build(&LayoutBox)` обходит layout pre-order и собирает четыре независимых дерева (Transform / Scroll / Effect / Clip). Триггеры: TransformNode — `transform != []` (локальная Mat4 = T(origin)·M·T(-origin)); ScrollNode — `overflow-x/y != visible`; EffectNode — `opacity<1` ∨ `filter` ∨ `mix-blend-mode != normal` ∨ `isolation: isolate`; ClipNode — `clip-path` ∨ `overflow-x/y` clipping. Mat4 расширен 2D-builders (translation/scale/rotate/skewX/skewY/matrix) + column-major multiply + **`invert_2d_affine()`** (через det(a·d - b·c); сингулярные → None) + **`transform_point_2d(x, y)`** (для hit testing P2 п.2B). Parent-граф каждого дерева — независимый (ближайший ancestor, который сам вкладывал узел в *это* дерево; иначе root). Анонимные InlineRun-ы пропускаются. P2 переходит с `PropertyTrees::build_stub()` на `::build()` без правок API. **P1 п.2A — Stacking contexts impl**: `StackingTree::build(&LayoutBox)` обходит layout pre-order и собирает SC по CSS Positioned Layout L3 §9.10 (триггеры: `position: fixed|sticky` всегда; `relative|absolute` с явным z-index; `opacity<1`; `transform`/`filter`/`clip-path` ≠ none; `mix-blend-mode` ≠ normal; `isolation: isolate`; `will-change` с stacking-property). Дочерние SC sortируются stable по z (`auto` ≡ 0). `ComputedStyle` расширен: `position`, `z_index: Option<i32>`, `isolation`, `mix_blend_mode` (16 keyword-ов из CSS Compositing & Blending L1 §3.1) — non-inherited, парсеры + ветви CSS-wide keyword-ов. Анонимный InlineRun не учитывается как owner SC (защита от фантомных контекстов). **CSS Quirks Mode UA-rule для `<table>`**: `apply_quirks_table_reset` в `compute_style` читает `doc.mode()` и при `DocumentMode::Quirks` сбрасывает у `<table>` font / color / text-align / white-space к initial-values (как в Chromium/Firefox/WebKit). В Standards / LimitedQuirks не применяется. Author CSS поверх — выигрывает. Отложено до появления table/inline-block layout: table cell width quirk, IE7 line-height quirk, unitless length / hashless hex color HTML-attr quirks, body propagation, flex/grid, float, абсолютное позиционирование, font-weight/style на inline-уровне
- 🟡 `lumen-paint` — display list (FillRect, **DrawBorder**, DrawText, **DrawImage**) + wgpu-растеризатор с двумя pipeline-ами (fill + text), **multi-size glyph atlas 1024×1024** (SIZE_BINS = [8,12,16,20,24,32,48,64]; ключ `(face_id, glyph_id, size_bin)`; растеризация на bin-подобранном размере без блюра при совпадении font-size с bin-ом), текстурированные квады из atlas-а. `DrawBorder` рендерится 4 fill-quad-ами (top/right/bottom/left edges), цвет с currentColor fallback. Под/над/перечёркивающие линии text-decoration эмитятся как FillRect-ы у baseline каждого фрагмента. `FontMeasurer` для TextMeasurer. **`lumen-paint::hit_test` (P2 п.2B)** — `hit_test(point, &LayoutBox) -> Option<HitTestResult { node, local_point, path }>`, обратный CSS Painting Order traversal (positive-z SC desc / in-flow + auto-0-z reverse-DOM / negative-z SC desc), фильтры `pointer-events: none` / `display: none`, transform inversion через `Mat4::invert_2d_affine()` (сингулярные forward-матрицы → бокс unhittable). Внешние зависимости: `wgpu` (exception #2), `winit` (exception #1)
- 🟡 `lumen-font` — собственный TrueType-парсер (head/maxp/cmap format 4+12/hhea/hmtx/loca/glyf/**fvar**/**avar**/**HVAR**/**VVAR**/**MVAR**/**gvar**) + scanline-растеризатор (квадратичные Безье, 4×4 AA, even-odd fill). cmap format 12 — Sequential Groups, полный Unicode U+10FFFF (эмодзи U+1F600+, SMP). **`fvar` parser (Variable Fonts L1 enabler)** — `Font::fvar() → Fvar { axes: Vec<VariationAxis { tag, min, default, max, flags, name_id }>, instances: Vec<NamedInstance { subfamily_name_id, flags, coordinates, post_script_name_id }> }`; named instances («Regular» / «Bold» / «Light Italic») — фиксированные точки в пространстве осей для UI font picker-а; `Fvar::instance_by_name_id(id)` lookup. **`avar` parser (axis normalization)** — `Font::avar() → Avar { segments: Vec<SegmentMap { maps: Vec<AxisValueMap { from, to }> }> }`, `Avar::normalize(axis_index, coord)` применяет piecewise-linear перенормализацию. **Composite glyphs ARGS_ARE_XY_VALUES=0 (point alignment)** — `Anchor` enum (`Offset(dx, dy)` / `Points { parent, child }`); `glyph_resolved` в point-mode вычисляет смещение как `parent.point[args1] − transformed_child.point[args2]` (рудиментарное TrueType-выравнивание pre-1996). **`ItemVariationStore` parser** — общий enabler для HVAR/MVAR/gvar: `VariationRegionList` (tent-функции на осях через F2DOT14 start/peak/end) + `ItemVariationData` blocks (region indexes + per-item delta_sets со смешанным i16/i8 storage). Format 1 only. **`DeltaSetIndexMap` parser** — HVAR/VVAR/MVAR glyph_id → (outer, inner) lookup: format 0 (16-bit map_count) и format 1 (32-bit), entry packed в 1..4 байта с настраиваемой inner-bit-разделкой. Per-spec out-of-range index → последняя entry. **`HVAR` parser** — `Font::hvar() → Hvar { store, advance_width_map, lsb_map, rsb_map }`, `advance_width_index(glyph_id)` через map или identity fallback (outer=0, inner=glyph_id) per spec. **`VVAR` parser** — `Font::vvar() → Vvar { store, advance_height_map, tsb_map, bsb_map, v_org_map }` — зеркало HVAR для вертикальных метрик (CJK vertical, Mongolian); identity fallback по advance_height, для TSB/BSB/vOrg отсутствующая map = «нет вариаций» (caller проверяет `has_*_variations()`). **`MVAR` parser** — `Font::mvar() → Mvar { store, records: Vec<ValueRecord { tag, delta_set_outer, delta_set_inner }> }`, `Mvar::lookup(tag)` O(log n) bin-search для standard metric tags (`xhgt`/`cpht`/`undo`/`unds`/`strs`/sub-super-script/ascender/descender). `ItemVariationStore::evaluate(outer, inner, coords)` ✅ (tent-function runtime: per-axis scalar + product по region-у, sum по data block). **`gvar` parser** ✅ — `Font::gvar() → Gvar<'a> { axis_count, shared_tuples, glyph_count, flags, glyph_data, glyph_offsets }` (lazy per-glyph: `glyph_variation_data(glyph_id) -> Option<&[u8]>`, `parse_glyph(glyph_id) -> Result<Option<GlyphVariationData>>`); поддержка short/long offsets через `flags & FLAG_LONG_OFFSETS`. `GlyphVariationData { tuple_variations: Vec<TupleVariation { peak, intermediate: Option<(start, end)>, points: PointNumbers::{All|Explicit(Vec<u16>)}, x_deltas: Vec<i16>, y_deltas: Vec<i16> }> }`. Реализованы packed point numbers (count 1/2-byte, runs с word/byte deltas, cumulative), packed deltas (zero / word / byte run-encoding), embedded peak / shared peak tuple lookup, intermediate region, private/shared/all point modes. Runtime `tuple_axis_scalar(coord, peak, intermediate)` + `tuple_scalar(coords, &variation)` — tent-функция с дефолтным region-ом из peak. Осталось: IUP (Interpolation of Untouched Points) и применение deltas к outline + 4 phantom-points в rasterizer-е; integration с CSS `font-variation-settings` cascade. 262 unit + 10 integration тестов. Отложено: hinting, GSUB/GPOS shaping, CFF outlines, integration variable-font deltas в rasterizer (IUP + application), color glyphs, composite flags USE_MY_METRICS / OVERLAP_COMPOUND / SCALED_COMPONENT_OFFSET.
- 🟡 `lumen-encoding` — детектор кодировок и декодеры: **UTF-8, UTF-16 LE/BE, Windows-1251, KOI8-R, CP866**. Пайплайн: BOM (UTF-8/UTF-16 LE/UTF-16 BE) → `<meta charset>`-sniff (1 КБ) → HTTP content-type hint → UTF-8 валидность → частотная эвристика по русским буквам. UTF-16 декодер обрабатывает surrogate-пары (BMP + supplementary через U+10000+), lone surrogates и нечётное число байт → U+FFFD. Реализует `EncodingDetector` из `lumen-core::ext`. 59 тестов (включая UTF-16 surrogate-пары, emoji, ASCII/cyrillic в обоих endian). Отложено: ISO-8859-5, MacCyrillic, prescan по HTML5 spec §12.2.3.2 (точные правила парсинга атрибутов)
- 🟡 `lumen-image` — собственный декодер растровой графики. **PNG-декодер** для Gray / GrayAlpha / RGB / RGBA при `bit_depth ∈ {8, 16}` (16-bit downsample-ится до 8 бит на канал отбрасыванием младшего байта — эквивалент `PNG_TRANSFORM_STRIP_16` в libpng) + **palette (color_type 3) с опц. tRNS + 1/2/4-bit grayscale и palette** (sub-byte unpack + scaling по PNG §13.12) + **tRNS для non-palette grayscale/RGB (single-color transparency, Gray8→GrayAlpha8 / Rgb8→Rgba8 с бинарным match-ом)** + **Adam7-interlacing для всех color types** (decode_adam7 — 7 passes раскладываются в финальный row-major буфер): свой CRC32 (IEEE 802.3 reflected), chunk reader, IHDR + PLTE + tRNS parsers, DEFLATE/inflate (RFC 1951: stored/fixed/dynamic Huffman, LZ77 окно 32 КБ), zlib-обёртка (RFC 1950 + adler-32), развёртка фильтров скан-линий (None/Sub/Up/Average/Paeth), bit-unpacking MSB-first, grayscale-масштабирование (1-bit×255, 2-bit×85, 4-bit×17), расширение палитровых индексов в Rgb8 / Rgba8. **JPEG baseline (SOF0) + progressive (SOF2)** декодер (ISO/IEC 10918-1): 8-bit precision, Y-only grayscale → Gray8 и 3-component YCbCr → Rgb8 через ITU-R BT.601 (JFIF §7); chroma subsampling 4:4:4 / 4:2:2 / 4:2:0 с nearest-neighbour upsampling; canonical Huffman (Annex C) build с Kraft-McMillan валидацией; bit reader с byte-stuffing `FF 00 → FF` и остановкой на маркерах; прямой 2D IDCT 8×8 в фиксированной точке ×1024; restart intervals (DRI + RST0..RST7 с циклическим счётчиком и сбросом DC predictors); marker reader для SOI/EOI/SOF0/SOF2/DHT/DQT/SOS/DRI/APPn/COM. **Progressive multi-scan loop** (§G): coefficient buffers per-component, 4 типа scan — DC initial (`<< Al`), DC refinement (1 бит → позиция Al), AC initial (RLE+EOBn extension `<< Al`), AC refinement (§G.1.2.3 — refine non-zero 1 битом, новые non-zero вставляются между ними; ZRL пропускает 16 zero-positions; EOBn вводит EOB-run mode); переопределение Huffman-таблиц между scan-ами через `read_next_segment_after_scan`; финализация (dequantize + IDCT + upsample) после EOI. Прочие SOFn (extended/lossless/hierarchical/arithmetic) и DAC отвергаются. Никаких сторонних crate-ов (см. §5). 130 unit + 57 integration тестов на реальных PNG/JPEG-фикстурах (включая progressive 4:4:4 / 4:2:0 / grayscale + gradient-ы + jpegtran-сгенерированный полный DC-refinement scan-script). Отложено: 12-bit / CMYK / ICC из APP2, WebP, AVIF — отдельными задачами.
- ✅ `lumen-network` — HTTP/1.1 + HTTPS клиент (rustls, exception #3). Redirect, chunked TE с **корректным дочитыванием trailer-секции** (без этого keep-alive ломался — хвост старого ответа попадал в следующий status-line). URL берётся из `lumen_core::url::Url` — никакого собственного парсинга здесь нет; **IDN-домены** конвертируются в Punycode через `Url::host_ascii()` непосредственно перед TCP/TLS/Host-header (DNS/TLS SNI/Host получают `xn--…` форму). `HttpClient` реализует `NetworkTransport`. **EventSink-интеграция** (принцип №4 «каждый исходящий байт виден»): `HttpClient::with_sink/with_tab` builder, эмит `RequestStarted` перед сокетом и `RequestCompleted` после получения статуса — для каждого редирект-хопа отдельная пара событий. Редирект-Location резолвится через `Url::resolve` (RFC 3986 §5.3). **RequestFilter hook** (`with_filter`): per-URL `should_block` проверяется до RequestStarted и до TCP; на блок эмитится `RequestBlocked { reason }` (а не Started/Completed) и `fetch` возвращает Err. **HTTP/1.1 keep-alive + ConnectionPool** (`HttpClient::with_pool` или собственный по умолчанию): `Connection: keep-alive` в request-header, после успешного ответа TCP/TLS-соединение возвращается в Mutex<HashMap<(host,port,is_tls), Vec<Entry>>> с timestamp; следующий запрос к тому же origin переиспользует idle (LIFO, idle_timeout=30 с, max_idle_per_host=6); `Connection: close` от сервера, EOF или ошибка чтения трактуются как `closed` и не идут в пул; **retry-on-stale** — при попадании на закрытое сервером idle-соединение клиент один раз перезапускает запрос на свежем connect-е (детектится по io::ErrorKind::BrokenPipe/ConnectionReset/UnexpectedEof + наши EOF-сообщения). **DnsResolver hook** (`with_dns_resolver`): resolve вынесен в trait-точку из `lumen-core::ext`, default = `SystemDnsResolver` (через `(host, port).to_socket_addrs()`), подменяется на `CachedDnsResolver` (lumen-storage) для TTL-кеша или на `DohResolver` (см. ниже); connect двухэтапный — `resolver.resolve()` → try-each `SocketAddr` до первого успешного `TcpStream::connect`. Per redirect-hop вызывается независимо. **DoT resolver** (`lumen_network::DotResolver`, RFC 7858): реализует `DnsResolver` поверх собственного TCP+TLS-сокета (rustls, exception #3, без HTTP). Конструктор `DotResolver::new(server_name, server_addr)` принимает pre-resolved IP+порт DoT-сервера (bootstrap-разделение от system DNS); фабрики `cloudflare()` / `google()` / `quad9()` зашивают hardcoded IP-литералы `1.1.1.1` / `8.8.8.8` / `9.9.9.9` на порт 853 (`DOT_DEFAULT_PORT`). На каждый `resolve` шлёт AAAA+A последовательно, IPv6 prefer (RFC 6724 §6), свежий TLS handshake per query (Phase 0; persistent — отложено). Wire-format переиспользует `doh::encode_query` / `doh::decode_answer_ips` (теперь `pub`); сверху — собственный TCP framing (RFC 1035 §4.2.2 `[u16 BE length][message]`) через `frame_query` / `read_framed_message`. `query_over_stream<S: Read+Write>` — generic exchange-функция, тестируется через mock `Cursor<Vec<u8>>` без поднятия TLS. IP-литералы bypass. **DoH resolver** (`lumen_network::DohResolver`, RFC 8484): реализует `DnsResolver` поверх произвольного `NetworkTransport` (типично — собственный `HttpClient` с bootstrap-резолвером); на каждый `resolve` шлёт два GET (AAAA + A, объединяет с IPv6 prefer по RFC 6724 §6) с `?dns=<base64url(query)>` к DoH endpoint-у; собственный DNS wire-format encoder/decoder (RFC 1035 §4 — header/flags/labels/compression pointers §4.1.4, A/AAAA RDATA, CNAME пропускается); собственный base64url без padding (RFC 4648 §5); IP-литералы (`8.8.8.8`, `::1`, `[::1]`) bypass без обращения к endpoint-у; RCODE≠0 / TC=1 → Err. **HSTS enforcement** (`with_hsts`, RFC 6797): trait-точка `HstsEnforcement` (lumen-core::ext), реализация — `lumen-storage::hsts::HstsStore`, fail-open. Pre-request §8.3 — http→https upgrade для known-hosts с правильной port-mapping (явный :80 убирается, custom-port сохраняется); upgrade ДО RequestFilter/RequestStarted — observer и блок-листы видят upgraded URL. Post-response §8.1 — парсинг `Strict-Transport-Security` только из HTTPS-ответов (HTTP STS игнорируется как небезопасный), `max-age=0` сохраняется как «снять HSTS». Каждый redirect-hop проверяется независимо. **HTTP Range requests** (RFC 7233, `fetch_range(url, RangeSpec, Option<RangeValidator>) -> Result<RangeResponse>`): single-range запросы в трёх формах — closed `bytes=START-END`, open-ended `bytes=START-`, suffix `bytes=-N` (последние N байт); опциональный `If-Range` validator (ETag / Last-Modified, дословно копируется в header). 206 ответ парсит `Content-Range` в типизированный `ContentRange {start, end, total: Option}`, 200 фолбэк (server игнорирует Range или If-Range mismatch — ресурс изменился) даёт full body + content_range=None, 416/4xx/5xx → Err. Range + If-Range пробрасываются в redirect-target. Phase 0: no multi-range/multipart (для `<video>` seek — отдельной задачей). **HTTP auth — RFC 7235 + 7617 Basic + 7616 Digest** (`with_credentials(Arc<dyn HttpCredentialProvider>)`): на 401 + `WWW-Authenticate` парсим challenge-list (token / quoted-string с `\` escape, корректное различение «,» как separator challenges vs auth-param), выбираем strongest (Digest+SHA-256 > Digest+MD5 > Basic), формируем `HttpAuthChallenge { origin, realm, scheme }`, опрашиваем `HttpCredentialProvider`, собираем `Authorization`: Basic = `base64(user:pass)`, Digest = HA1 / HA2 / response по MD5 / MD5-sess / SHA-256 / SHA-256-sess (qop=auth + RFC 2069 legacy без qop); собственные MD5 (RFC 1321) + SHA-256 (FIPS 180-4) — не security-критично (Digest = challenge-response, не KDF). Один retry на hop, Authorization не пересылается через 3xx (RFC 7235 §3.1). `StaticCredentialProvider` для тестов / `-u user:pass`. 194 тестов (+38 DoH: 8 encode + 11 decode + 5 base64url + 10 resolver integration через mock-NetworkTransport + 4 service; +22 DoT: 4 frame_query + 5 read_framed_message + 5 query_over_stream через mock Read+Write + 3 IP-литерал bypass + 5 service; +49 HTTP auth: 8 hash vectors RFC 1321/FIPS 180-4 + 2 base64-std + 9 parser + 4 select + 8 builder + 11 mock-server retry; +21 Content-Encoding: 8 BrotliContentDecoder unit + 8 apply_content_encoding unit + 2 builder + 3 e2e через mock-TcpListener). **Content-Encoding pipeline**: `HttpClient::with_content_decoder(Arc<dyn ContentDecoder>)` регистрирует декодер; `Accept-Encoding` запроса собирается из имён (порядок регистрации = порядок предпочтения); `Content-Encoding` ответа парсится (case-insensitive, comma-separated), `identity`/пустые токены пропускаются, encoding без декодера → Err; для stacked encodings декодеры применяются в обратном к header-у порядке (RFC 7231 §3.1.2.2). **`BrotliContentDecoder`** (`crates/network/src/brotli.rs`) — реализация `ContentDecoder` (encoding `"br"`) поверх **provisional `brotli-decompressor` = "5"** (RFC 7932, §5 «Provisional accelerators»), trait-anchor — `ContentDecoder` в `lumen-core::ext`, graduation criterion — реалистично никогда (формат стабилен с 2016). **Mixed-content enforcement** (W3C Mixed Content §5, P3 2A шаг b): `MixedContentPolicy { top_level: Origin, mode: MixedContentMode { Disabled | SpecDefault | Strict } }` + builder `with_mixed_content_policy(top_level, mode)` + публичный `fetch_subresource(url, destination) -> Result<Vec<u8>>`. Classify+block в `fetch_with_redirect` ПОСЛЕ HSTS upgrade и ДО `RequestFilter` / `RequestStarted`: blockable mixed-content (scripts / styles / iframes / fonts / fetch / worker) в SpecDefault и в Strict; OptionallyBlockable (images / media / prefetch) — только в Strict. Event::RequestBlocked с reason формата `"mixed-content: blockable" | "mixed-content: optionally-blockable"`, per redirect-hop. `NetworkTransport::fetch(url)` (top-level navigation) — без destination и enforcement. **CORS preflight classifier + cache** (Fetch §3.2.2 — §4.10, P3 2A continuation): pure-логика в `lumen-network::cors` без интеграции в HttpClient. `is_cors_safelisted_method` (GET/HEAD/POST), `is_forbidden_request_header` (exact + `sec-`/`proxy-` префиксы), `is_cors_safelisted_request_header` (Accept/Accept-Language/Content-Language/Content-Type/Range с essence-парсингом и 128-байт-лимитом). `CorsRequest { origin, target, method, headers, credentials_mode }`, `CredentialsMode { Omit, SameOrigin (default), Include }`. `needs_preflight(req)` (method не safelisted ∨ хоть один header не safelisted). `unsafe_request_header_names` — lowercased, deduplicated, sorted (Fetch §4.8 step 7.1). `build_preflight_headers` — `Origin`/`Access-Control-Request-Method`/`Access-Control-Request-Headers`. `PreflightResult { allowed_methods, allowed_headers, allow_credentials, max_age_seconds }` + `method_allowed` (`*` wildcard + safelisted-implicit) + `unmatched_header` (`*` wildcard кроме `Authorization`). `evaluate_preflight_response(status, headers, req)` — status 200-299, ACAO (validate exact-match через `Origin::serialize` или `*`-если-не-Include), ACAC (`true` обязателен при Include), ACAM, ACAH, ACMaxAge (default 5 сек, invalid → Err). Финальная проверка actual method+headers против allow-lists. `check_cors_response_headers(headers, origin, mode)` — отдельный entry для actual response (ACAO+ACAC, без ACAM/ACAH). `PreflightCache` — thread-safe (`Mutex<HashMap>`), ключ `(requestor_origin, target_origin, credentials_mode)`, TTL = `max_age_seconds`, lazy-expire на lookup, `allows_at(&req, now)` shortcut. 56 unit-тестов. Этот модуль — **классификатор и спецификация, не enforcer**; реальная отправка OPTIONS + cache-hooks в `HttpClient::fetch_with_redirect` — следующая задача (по аналогии с mixed-content split: classifier → enforcement).
- ✅ `lumen-storage` — два бэкенда `StorageBackend`: in-memory KV с snapshot LUMEN_KV_V1 (для тестов / ephemeral) + **SqliteStorage** (persistent, через `rusqlite` bundled — exception #5; WAL + synchronous=NORMAL; одна таблица `kv` с composite PK). Полное origin-партиционирование в обоих. **CookieJar** — RFC 6265 / RFC 6265bis cookies поверх SQLite: domain/path matching, expires_at TTL, secure-only-HTTPS, SameSite (Strict/Lax/None), top_level_site partitioning для total cookie protection (§9.2); `parse_set_cookie_with_psl` применяет RFC 6265bis §5.5 step 5 — public-suffix защита Domain attribute (super-cookie reject + host-only fallback). **History** — посещённые страницы (url/title/visit_date/visit_count/favicon_hash/text_sha256) с upsert-semantics и API recent/most_visited — основа под §12.1 полнотекстовый поиск. **CachedDnsResolver** (`lumen-storage::cached_dns`) — реализация `DnsResolver` поверх `DnsCache`: оборачивает произвольный inner-resolver (system / DoH в будущем), на каждый `resolve` сначала пытается hit по кэшу (с TTL и порт-подстановкой на каждый вызов — порт не кэшируется), при miss идёт в inner и `cache.put` с `default_ttl_seconds`. `Clock` trait для подмены времени в тестах. **SafeBrowsingList** (`lumen-storage::safe_browsing`) — локальный аналог Google Safe Browsing v4 без облачного API (принцип №1): таблица `safe_browsing(list_name, full_hash BLOB(32), threat_type, added_at)` с composite PK + index по full_hash; ThreatType { Malware / SocialEngineering / UnwantedSoftware / PotentiallyHarmful / Other(_) }; canonicalize URL + 5 host-suffix × 4 path-trim вариантов на запрос; **`SafeBrowsingFilter::with_psl`** — host-suffix enumeration обрезается до eTLD+1 (через `PublicSuffixList`), что блокирует ложно-широкие матчи через shadow-entry на public suffix; `SafeBrowsingFilter` реализует `RequestFilter`, fail-open на ошибки lookup. **`PslProvider`** (`lumen-storage::psl`) — реализация `PublicSuffixList` через provisional **`psl = "2"`** (compiled-in таблица, codegen из public_suffix_list.dat на этапе сборки). 435 тестов.
- 🟡 `lumen-knowledge` (§12) — базовая FTS5-таблица `history_fts(url, title, text)` поверх SQLite с tokenizer `unicode61` и bm25-ранжированием готова. **§12.2 заметки** (`Notes` с external content FTS5 `notes_fts(selection, comment)` и triggers для авто-sync) и **§12.3 read-later** (`ReadLater` с html_snapshot BLOB, status, tags + external content FTS5 + триггеры) готовы. API: index/unindex/search для HistoryFts; add/update/delete/list_for_url/recent/search для Notes; save/set_status/touch/get/list_by_status/search для ReadLater. 39 тестов. Отложено: §12.4 поиск по открытым вкладкам, §12.2 Range API для highlight-наложений, Porter-stemmer для русского, §12.3 фоновый downloader для ресурсов при save.
- ⬜ `lumen-ai` (§12.5) — опциональный, embedding + RAG поверх локального LLM. Phase 3+, feature-flag

### Политика зависимостей (§5, обновлена 2026-05-15)
- ✅ Зафиксирована (две категории, см. §5): **Permanent exceptions** — никогда не пишем сами; **Provisional accelerators** — берём готовое сейчас, заменяем по событию. Ядро (HTML/CSS/DOM/layout/paint/font/encoding/URL/HTTP/1.1+2/DNS/adblock/knowledge/UI) — всегда наше.
- ✅ Permanent #1: `winit` (OS event loop) — за `WindowingBackend`
- ✅ Permanent #2: `wgpu` (GPU API) — за `RenderBackend` — активирован в `lumen-paint`
- ✅ Permanent #3: `rustls` + `webpki-roots` (TLS / crypto + Mozilla CA bundle) — за `TlsBackend` — активирован в `lumen-network`
- ✅ Permanent #4: SQLite (`rusqlite` с `bundled`) — за `StorageBackend` + `KnowledgeStore` — активирован в `lumen-storage` и `lumen-knowledge`
- ⬜ Permanent #5: JS engine (`rquickjs` → `rusty_v8`) — за `JsRuntime` — пока не подключён
- 🟡 Provisional (2 подключено: `brotli-decompressor` в `lumen-network` через `BrotliContentDecoder` за `ContentDecoder`; **`psl`** в `lumen-storage` через `PslProvider` за `PublicSuffixList` — RFC 6265bis §5.5 cookies + Safe Browsing host-suffix). Ожидают подключения: image decoders (JPEG/WebP/GIF), `icu4x`, `ruzstd`, `idna`, `hyphenation`, `woff2`, `hunspell-rs`/`spellbook`, `quinn`. Каждый — за trait в `lumen-core::ext`, подключается по мере того, как фаза реально упирается в задачу. Полная таблица + graduation criteria — в §5.

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

### Уникальные фичи (§12) — план на Phase 1-4
- ⬜ Tab session export / import (§12.7) — Phase 1
- 🟡 Полнотекстовый поиск по истории (§12.1) — FTS5 + bm25 готовы в `lumen-knowledge::HistoryFts`; осталась интеграция с shell (omnibox `@history` префикс) и Porter-stemmer для русского
- 🟡 Аннотации и заметки (§12.2) — `lumen-knowledge::Notes` storage layer готов; Range API для восстановления highlight-наложений на странице — отложено
- 🟡 Read-later / офлайн-чтение (§12.3) — `lumen-knowledge::ReadLater` storage layer готов (status, tags, FTS5); фоновый downloader для ресурсов при save и UI отложены
- ⬜ Поиск по содержимому открытых вкладок (§12.4) — Phase 2
- ⬜ Focus mode (§12.6) — Phase 2
- ⬜ Кастомизация UI (drag&drop, темы) (§12.10) — Phase 2-3
- ⬜ Локальный AI layer (§12.5) — Phase 3+, опционально
- ⬜ Семантические закладки (§12.8) — Phase 3, зависит от AI
- ⬜ Граф знаний (§12.9) — Phase 3+
- ⬜ Кросс-устройственная синхронизация E2E (§12.11) — Phase 4+, требует mobile

### Локализация / RU (§10)
- ✅ DOM держит кириллицу (UTF-8) — зафиксировано тестами
- ✅ `Url::parse` принимает кириллические домены (тест на `президент.рф`)
- ✅ Encoding detection (cp1251, KOI8-R, CP866) — крейт `lumen-encoding`, подключён в shell
- ⬜ Cyrillic font fallback в paint
- ✅ Punycode/IDN — `lumen_core::punycode` (RFC 3492 encode) + `lumen_core::idn::domain_to_ascii`; `Url::host_ascii()` отдаёт ASCII-форму host для DNS/TLS/Host header — единственная точка вызова `idn::domain_to_ascii` среди потребителей
- ⬜ Локаль ru-RU (дата/время/числа)
- ⬜ UI-переводы (Fluent)

### Следующие шаги
- 🟡 HTML parser — минимум готов; полный набор insertion modes / named entities / DOCTYPE-разбор — позже, по запросу
- 🟡 CSS parser — селекторы готовы (compound, combinators, attribute, structural+functional pseudo, `:not`, specificity); типизированные значения (length/color/calc), `:is/:where/:has` — позже
- 🟡 Layout — block-flow + inline-flow + style cascade (specificity) + word-wrap готовы; flex/grid, float, абсолютное позиционирование — позже
- ✅ Paint — display list + wgpu-rasterizer + glyph atlas + text rendering
- ✅ Связка движка с UI: shell открывает `samples/page.html` с фонами и текстом
- 🟡 lumen-image — PNG (8/16-bit + palette + tRNS + Adam7) и JPEG baseline (DCT/Huffman/YCbCr) декодеры готовы; интеграция в layout/paint (`<img>` block-level placeholder сделан) и WebP/AVIF — отдельными задачами
- ⬜ Composite glyphs в lumen-font (Cyrillic 'А' и другие)
- ⬜ Свой HTTP/1.1 + TLS через `rustls` — для загрузки внешней страницы

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

| # | Задача | Что разблокирует | Что НЕ блокирует |
|---|---|---|---|
| 1A | 🟡 **`[P1]` Quirks-mode application на hot-paths cascade/layout.** Detection готов (`DocumentMode` enum + `quirks_mode::detect_document_mode`). **Реализовано в ветке `quirks-mode-application`**: `apply_quirks_table_reset` в `compute_style` сбрасывает font/color/text-align/white-space у `<table>` в Quirks-mode к initial-values (эквивалент UA-stylesheet rule в Chromium/Firefox/WebKit). Author CSS поверх — выигрывает. **Реализовано в ветке `quirks-hashless-hex-color`** (CSS Quirks Mode §3.4): `parse_color_legacy(s, is_quirks)` — в Quirks-mode bare hex digits длиной 3/6/8 без ведущего `#` парсятся как color (например, `color: ff0000` → red). Применяется ко всем CSS `<color>`-полям через cascade pipeline (color/background-color/border-color/outline-color/text-decoration-color/caret-color/accent-color/box-shadow/text-shadow/scrollbar-color и shorthands). В Standards/LimitedQuirks тождественно отвергает hashless форму. **Реализовано в ветке `html-legacy-bgcolor`** (HTML5 §2.4.6 «rules for parsing a legacy color value» + §15 «Rendering»): `parse_legacy_color_html_attr` — лояльный парсер для presentational-hint атрибутов (named colors, `#rgb`/`#rrggbb` short+long, hashless hex произвольной длины через padding/truncate procedure, non-BMP→«00», unhex→«0»; отказы только на пустую строку и `transparent`). `apply_bgcolor_presentational_hint` мапает `bgcolor` на `background-color` для `<body>` / `<table>` / `<thead>` / `<tbody>` / `<tfoot>` / `<tr>` / `<td>` / `<th>` ДО CSS-каскада — author CSS перекрывает hint. Это закрывает «hashless hex color quirk **в HTML атрибутах**» из списка ниже (отличается от CSS-quirk выше: HTML legacy работает независимо от document mode, и алгоритм совершенно другой). 16 unit-тестов на парсер + 8 на integration. **Реализовано в ветке `html-legacy-text-color`** (HTML5 §15.3.6 + §15.3.2): `apply_text_color_presentational_hint` мапает `<body text="…">` на `body.color` (через CSS inheritance красит потомков) и `<font color="…">` на `color` элемента. Используется тот же `parse_legacy_color_html_attr`. Author CSS поверх — выигрывает. 12 unit-тестов на integration. `<body link/vlink/alink>` отложены: hyperlink coloring требует UA-правил с descendant-селектором (`body :link { color: … }`), а в Phase 0 без `:visited`/`:active` runtime два из трёх атрибутов всё равно были бы no-op. Остаётся (требует table layout / inline-block): table cell width quirk (§4.1), IE7 line-height quirk для replaced (§3.2), unitless length quirk вне `<img>` (§3.3), body propagation quirk. | Половина legacy-сайтов перестаёт рендериться неправильно. | Никого. |
| 1B | **`[P1]` Типизированные `Length` / `Color` во всех декларациях каскада** (не строки). CSS Variables / `calc()` / `min/max/clamp` / scientific math / @property уже реализованы — осталось унифицировать хранение. | P2 п.3A (color management видит реальный `Color`), P3 (CSSOM JS bindings отдают типизированные значения). | P2/P3 уже видят типы из Sprint 0 — не блокирует. |
| 2A | ✅ **`[P1+P2]` Stacking contexts impl** в layout (наполнить `StackingContextId` от Sprint 0 реальной логикой). Stacking context создают: `opacity<1`, `transform != none`, `filter`, `clip-path`, `isolation: isolate`, `mix-blend-mode != normal`, `will-change` с stacking-property, `position: fixed/sticky` всегда, `position: relative/absolute` с явным z-index. **Реализовано в ветке `stacking-contexts-build`** (`StackingTree::build` + 4 новых свойства в ComputedStyle). Flex/grid item с z-index отложено до реального flex/grid pass. | P2 п.2A (painting order traversal) — drop-in переход с пустого stub-а на реальные данные. | P2 уже пишет painting order против stub. |
| 2B | ✅ **`[P2+P1]` Property trees построение** (TransformTree / ScrollTree / EffectTree / ClipTree из style+layout). **Реализовано в ветке `property-trees-build`**: `PropertyTrees::build(&LayoutBox)` обходит layout pre-order и строит четыре независимых дерева. Триггеры: TransformNode — `transform != []` (с локальной матрицей через transform-origin); ScrollNode — `overflow-x/y != visible`; EffectNode — `opacity<1` ∨ `filter != []` ∨ `mix-blend-mode != normal` ∨ `isolation: isolate`; ClipNode — `clip-path` ∨ `overflow-x/y` clipping (Hidden/Clip/Scroll/Auto). Mat4 расширен 2D-builders (translation/scale/rotate/skewX/skewY/matrix) + column-major multiply. Анонимные InlineRun-ы пропускаются — то же правило, что в stacking. P2 теперь может перейти с `PropertyTrees::build_stub()` на `::build()` без правок API. | P2 п.1B (compositor commit). | P2 уже пишет compositor против пустых trees. |
| 3A | 🟡 **`[P1+P2+P3]` Web Animations interpolation** (cubic-bezier / steps timing → computed value в момент `t`, invalidation animated-свойств). Зависит от п.1B (типизированные Length/Color) для value-interpolation. **Foundation ready (ветка `animation-longhands`):** `TimingFunction` enum (Linear / CubicBezier / Steps + StepPosition) + parser (linear / ease / ease-in / ease-out / ease-in-out / cubic-bezier(...) / steps(...) / step-start / step-end); `animation-*` longhands в ComputedStyle (`animation_names` / `_durations` / `_timing_functions` / `_delays` / `_iteration_counts` (`IterationCount` enum: Finite/Infinite) / `_directions` (`AnimationDirection` enum) / `_fill_modes` (`AnimationFillMode` enum) / `_play_states` (`AnimationPlayState` enum)); `transition_timing_functions` тоже добавлен. Все 9 longhands — comma-list, не наследуются, initial = empty Vec. **Bezier solve + LinearInterpolator готовы (ветка `animation-interpolator`)**: `TimingFunction::progress(t)` (CSS Easing L1 §2 — Linear / CubicBezier через Newton-Raphson + bisection fallback / Steps все 4 step-position); `LinearInterpolator` в `lumen-layout::animation` — drop-in замена `NoopInterpolator` для Number/Length(same-unit)/Color, fallback to step-half для Discrete/mixed-unit/Calc/cross-type pairs. **Transform-list interpolation готова (ветка `transform-interpolate`)** — CSS Transforms L2 §15: `AnimValue::TransformList(Vec<TransformFn>)` + два пути в `interpolate_transform_list`: (1) matched-pair lerp при одинаковой длине и совместимых `TransformFn` variants на каждой позиции (Translate / TranslateX / TranslateY / Rotate без shortest-path / Scale / ScaleX / ScaleY / SkewX / SkewY / Matrix-pair с внутренним decompose); (2) 2D matrix decompose fallback для mismatched / `none`-стороны / Matrix-pair: оба `Vec<TransformFn>` композируются в 2D-аффинную `[a,b,c,d,e,f]`, декомпозируется в (tx, ty, scale_x, scale_y, skew, rotation) по §15.6 — с обработкой reflection (`det<0`) и сингулярных матриц (→ identity-decomp), shortest-path для rotation в decompose-пути, recompose `T*R*Sk*S`. 24 новых теста (matched-pair / matrix-fallback / decompose round-trip / shortest-path). 62 + 24 = 86 тестов на animation. **`animation` shorthand parsing готов (ветка `animation-shorthand`)** — CSS Animations L1 §4: `apply_animation_shorthand` парсит comma-list `<single-animation>#`, где `<single-animation>` — `||`-комбинация 8 sub-value-ов (duration / easing / delay / iter-count / direction / fill-mode / play-state / name) в любом порядке. `tokenize_with_parens` разрезает по whitespace с уважением к скобкам (`cubic-bezier(0.42, 0, 0.58, 1)` и `steps(4, end)` — один токен). `parse_single_animation` пробует каждый токен по slot-ам в фиксированном порядке (time → easing → iter → dir → fill → play → name), fall-through к name-кандидату; первое `<time>` → duration, второе → delay. Shorthand сбрасывает все 8 longhand Vec-ов и заполняет одну запись на каждый layer (initial-value для непроставленных slot-ов) — обеспечивает совпадение длин Vec-ов после развёртки. 28 новых unit-тестов (single-name / duration+name / full canonical / любой порядок / cubic-bezier-spaces / steps-args / multi-layer / parallel-lengths / none / negative-delay / ms-units / iter-fractional / shorthand-resets-longhands / direction-reverse / fill-both / play-paused / step-start / through apply_declaration). 86 + 28 = 114 тестов на animation. **`transition` shorthand parsing готов (ветка `transition-shorthand`)** — CSS Transitions L1 §3: `apply_transition_shorthand` парсит comma-list `<single-transition>#` (`[none | <single-transition-property>] || <time> || <easing-function> || <time>`); реиспользует `tokenize_with_parens` / `parse_time_seconds` / `split_top_level_commas` из animation-shorthand. Per-token classification fall-through по slot-ам: time → easing → property; первое `<time>` → duration, второе → delay; property — последний fallback (любой ident, включая `none`/`all`). Shorthand сбрасывает все 4 longhand Vec-а; `none` в позиции property сохраняется литеральной строкой `"none"` (не коллапсирует Vec в пустой, в отличие от longhand `transition-property: none`) — parallel-length-инвариант для consumer-а. 17 unit-тестов. 114 + 17 = 131 тест на animation/transition. **Filter-list interpolation готова (ветка `filter-list-interpolate`)** — CSS Filter Effects L1 §6: `AnimValue::FilterList(Vec<FilterFn>)` + `interpolate_filter_list(from, to, t) -> Option<Vec<FilterFn>>`. Алгоритм: пустой ↔ пустой = `none`; на common prefix проверяется матчинг типов `FilterFn` поэлементно; при mismatch — `None` (caller делает step-half). При совпадении типов в общем prefix-е, более короткая сторона достраивается lacuna (identity) значениями типов из более длинной: `Blur(0)`, `Brightness(1)`, `Contrast(1)`, `Grayscale(0)`, `HueRotate(0)`, `Invert(0)`, `Opacity(1)`, `Saturate(1)`, `Sepia(0)`. Это покрывает `filter: none ↔ filter: blur(10px)` (spec §6: «If only one filter is none, that side is treated as a list of identity filter functions»). Hue-rotate интерполируется линейно в радианах без shortest-path (parser хранит угол в радианах; spec не требует shortest-path для filter). Clamping значений в допустимый диапазон оставлено consumer-у. 15 новых unit-тестов (matched-pair single / multi / endpoints / kind-mismatch / prefix-mismatch / identity-padding обе стороны / hue-rotate radians / LinearInterpolator wrapping). 131 + 15 = 146 тестов на animation/transition. Осталось: gradient-stops interpolation, integration в animation scheduling (P3) и compositor offload (P2). | P2 п.3B (compositor offload для transform/opacity), P3 (animation scheduling в rendering steps stage) — оба читают ComputedStyle.animation_* и вызывают `progress(t)` + `LinearInterpolator::interpolate(...)` напрямую. | Stub interpolator из Sprint 0 уже давал компилируемый код; теперь реальный progress + linear + transform/filter-list interpolation на месте — P2/P3 видят интерпол-кадры transform/filter-анимаций из коробки. |
| ~~3B~~ | ✅ **`[P1+P3]` Push-tokenizer + incremental tree builder** в `lumen-html-parser`. **Реализовано в ветке `push-tokenizer`**: `PushTokenizer { feed, end }` поверх pull-Tokenizer-а с `find_safe_split`-эвристикой (учитывает `<!--…-->`, `<!DOCTYPE…>`, `<tag…>`, `&entity;` в data state и в RCDATA); pull-Tokenizer восстанавливает `text_only`-state при EOF; `IncrementalTreeBuilder { feed, finish }` поверх общего с `parse()` `apply_token`-helper-а с text-node coalescing. Инвариант: pull/push дают побайтово равный `Document`. 44 новых теста (29 push_tokenizer + 15 incremental_tree_builder) — property-тесты сравнивают byte-by-byte / chunk-by-8 / whole-input против pull-режима. UTF-8: caller отвечает за code-point boundary. **Осталось** (но не блокирует P3): `feed_bytes(&[u8])` с буферизацией partial UTF-8. | P3 п.4B (streaming pipeline — window-first init + async fetch chunks) разблокирован. | До п.3B P3 продолжал streaming-shell в blocking-режиме. |
| 4A | **`[P1+P2]` `<picture>` / `srcset` / `sizes` finishing.** 🟡 Готовы parser + pickers + `pick_picture_source` + **L4 nested `(not (...))` / `((...))` в media-condition** (ветка `media-query-nested-not`, `MediaClause::Nested` + paren-aware split). Осталось: IntersectionObserver event source для `loading="lazy"`. | P3 lazy-loading в shell. | Никого — P2 GPU upload не зависит от lazy. |
| 4B | **`[P1]` CSS Grid + полный Flexbox** в layout. | Современные адаптивные сайты. | Изолировано в `lumen-layout`. |
| 5 | **`[P1]` Подключить `icu4x.segmenter` + `icu4x.linebreak`** через `UnicodeProvider` (provisional, после Sprint 0). Правильный line-break для CJK / тайского / арабского. | Корректная типографика на не-Latin страницах. | Изолировано в `lumen-layout`. |
| 6+ | **`[P1+P3]` Shadow DOM cascade + composed tree + tree-builder для `<template>` / `<slot>`** (Phase 2, после JS engine у P3) · **`[P1+P3]` Accessibility tree construction** (Phase 2) · **`[P1+P3]` Forms ValidityState + validation pseudo-classes + submission algorithm** (Phase 2) · **`[P1+P3]` `<contenteditable>` DOM mutations + Selection / Range типы + `beforeinput`/`input`** (Phase 3) · **`[P1+P2+P3]` Print pagination algorithm** (Phase 3) · **`[P1+P3]` DOM-side wrapper hooks для GC integration** (Phase 3). | См. подробности в Browser fundamentals ниже. | После Sprint 0 эти задачи стартуют без ожидания других. |

#### Track P2 — Backend rendering

| # | Задача | Что разблокирует | Что НЕ блокирует |
|---|---|---|---|
| ~~1A~~ | ✅ **`[P2]` Font fallback / matcher.** Реализовано полностью: (a) picker `match_face` по CSS Fonts L4 §5.2 + OS/2 парсер + `FaceRecord` (`lumen-font::os2`, `lumen-core::ext::match_face`); (b) display-list plumbing — `DisplayCommand::DrawText` несёт `font_family`/`font_weight`/`font_style`; (c) real face switch — `Renderer` хранит `Vec<LoadedFace>` + `font_provider: Option<Arc<dyn FontProvider>>` (по умолчанию `SystemFontIndex`); `resolve_face_id` лениво грузит TTF; (d) per-char codepoint cascade в `push_text_glyphs` — если у primary face нет глифа, обходим loaded faces (CSS Fonts L4 §5.3). Осталось eager preload курируемого fallback-списка (Noto Color Emoji / Noto CJK) — это roadmap «Ближайшее» п.X, разблокирует тест-страницы с эмодзи / CJK без явного `font-family`. |  |  |
| 1B | 🟡 **`[P2+P1]` Compositor thread + layer tree scaffolding** против `PropertyTrees` от Sprint 0. Scaffolding ✅ (trait-ы `Layer` / `LayerTree` / `Compositor` + `BasicLayerTree::single_layer` + `InProcessCompositor` в `lumen-paint::compositor`). Two-buffer commit-модель ✅ (`commit` → pending; `flush_pending()` атомарно промотирует pending → active; `has_pending()` для invalidate-логики). DisplayCommand layer-ops definitions ✅ (`PushClipRect/PopClip`, `PushOpacity/PopOpacity`, `PushBlendMode/PopBlendMode` + `BlendMode` enum со всеми 17 режимами включая PlusLighter из L2). **Шаг (a) — layer-ops эмиссия в `build_display_list_ordered` ✅** (ветка `layer-ops-emission`): для боксов с `opacity<1` / `mix-blend-mode != Normal` / `overflow != Visible` эмитятся парные Push/Pop в LIFO-порядке (Clip → Blend → Opacity push, reverse pop). Фильтр `box_can_own_stacking_context` отсекает анонимные InlineRun-ы. SC-owner — Push/Pop в `bucket.pre`/`bucket.post`, non-SC (typically `overflow:hidden`) — inline в `contents`. Phase 0 ограничение: pre/post SC-owner-а не охватывают child-SC потомков (нужен либо stack-based emission, либо end-of-SC маркер в PaintOrder). Renderer пока игнорирует. **Шаг (b) — compositor thread ✅** (ветка `compositor-thread`): `ThreadedCompositor` + `ThreadedCompositorHandle` на `Arc<Mutex<ThreadedState>>` — owner реализует `Compositor` trait (`&mut self` для drop-in замены `InProcessCompositor`), `handle()` отдаёт cheap-clone handle с `&self` API для shared доступа из render/compositor threads. trait `Compositor` переведён с `Box<dyn LayerTree>` на `Arc<dyn LayerTree + Send + Sync>` и с `Option<&dyn LayerTree>`/`Option<&Arc<PropertyTrees>>` на cloned `Arc`-snapshot — обоснование: reference на поле внутри `Mutex` нельзя удерживать дольше lock guard-а; `Arc::clone` на возврате — O(1), lock сразу освобождается. Multi-thread тесты (`cross_thread_commit_and_flush`, `cross_thread_concurrent_commits_last_wins` через `Barrier`). Осталось: (a-cont) подключить `compositor.active_tree()` в shell-pipeline (P3 интеграция); (b-cont) реальный compositor-thread tick-loop с `JoinHandle` поверх `ThreadedCompositor` (читает pending каждые N мс, делает GPU upload); (c) реальный layer-pipeline для PushClipRect/PushOpacity/PushBlendMode (off-screen-layer composite + scissor). | Foundation для: off-main-thread scroll, п.3B (Web Animations compositor path), п.4 (mix-blend-mode/backdrop-filter pipeline), GPU process у P3. **Самый большой enabler в P2.** | Структура pipeline готова до того, как P1 закончит property trees — drop-in переход. |
| 2A | ✅ **`[P1+P2]` Painting order traversal** (CSS 2.1 Appendix E, 7-уровневый порядок). Реализовано в `PaintOrder::from_tree(&StackingTree)` — рекурсивный обход с правильным interleaving фаз parent-SC и child-SC по z-order: `RootBackground → neg-z children fully → BlockBackgrounds/Floats/InlineContent → auto/0-z children → positive-z children`. Renderer-сторона ✅ — `build_display_list_ordered(root, &StackingTree, &PaintOrder) -> DisplayList`: bucket-per-SC (root_bg + contents), child-SC рисуются в правильных слотах parent SC. Phase 0 lumps BlockBackgrounds/Floats/InlineContent в один contents bucket — точное разделение фаз 3/4/5 ждёт реального flex/float layout. **Shell-интеграция (P3):** заменить `build_display_list(&tree)` на `build_display_list_ordered(...)`. |  |  |
| 2B | ✅ **`[P2]` Stacking-aware hit testing.** Реализовано в ветке `stacking-hit-testing`: `lumen-paint::hit_test(point, &LayoutBox) -> Option<HitTestResult>` — обратный CSS Painting Order traversal с группами positive-z SC (desc по z) / in-flow+auto-0-z (reverse DOM) / negative-z SC (desc по z); фильтры `pointer-events: none`, `display: none`, `Skip`; transform inversion через `Mat4::invert_2d_affine()` для боксов с CSS `transform` (сингулярные → бокс не hittable). `HitTestResult.path` — ancestor chain для capture/bubble dispatch. **Интеграционная точка P3:** заменить простой layout-walk в shell input handler на `hit_test(mouse_pos, &layout_root)`. 14 unit-тестов + 9 на `Mat4::invert_2d_affine`/`transform_point_2d`. Phase 0 ограничения: фазы 3/4/5 (Block/Floats/InlineContent) лумпятся в один in-flow обход; InlineRun → `node = id родителя`; только 2D affine transforms (3D потребует 4×4 invert). |  |  |
| 3A | **`[P2]` Color management + Display P3 / Rec2020** через ICC profiles. Использует `Color` типы от P1 п.1B. | Фотографии с P3-профилем перестают выглядеть как sRGB. | Только `lumen-paint` — P1 типы уже из Sprint 0. |
| 3B | **`[P1+P2+P3]` Web Animations compositor offload** для `transform` / `opacity` (без main thread на frame). Использует `AnimationInterpolator` trait от Sprint 0; реальный impl приходит из P1 п.3A. | UX-видимая разница — smooth-анимации без блокировки main thread. | Stub interpolator уже даёт компилируемый код. |
| 4 | **`[P1+P2]` `mix-blend-mode` / `background-blend-mode` / `isolation` / `backdrop-filter` pipeline** на compositor (после п.1B). 16 blend modes, isolation groups в compositor pipeline, backdrop-filter — отдельный pass blur на снапшоте. | Современные UI-эффекты. | Только `lumen-paint::compositor`. |
| 5+ | **`[P1+P2]` `<img>` extras**: `object-fit` / `object-position`, inline-replaced, sRGB→linear pipeline · **`[P2]` GPU upload для `<picture>`/`srcset`** (после P1 4A) · **`[P2]` Canvas 2D** (Phase 3) · **`[P2]` WebFonts через WOFF2** через `FontFormat` от Sprint 0 (provisional `woff2` crate, Phase 2) · **`[P2]` Variable fonts axes runtime** (Phase 3) · **`[P1+P2+P3]` Print PDF generation** из display list (Phase 3). | См. fundamentals ниже. | Все после п.1B и п.1A — независимы от P1/P3. |

#### Track P3 — Runtime + system (объединённый домен — больше треков, но всё параллельно)

| # | Задача | Что разблокирует | Что НЕ блокирует |
|---|---|---|---|
| 1B | **`[P3]` `rquickjs` integration scaffold** в новом крейте `lumen-js` (только runtime + eval, без DOM bindings пока). Реализует `JsRuntime` trait от Sprint 0. Exception #4. | Самый большой single enabler в проекте. Разблокирует: P1 п.6+ (Forms / Shadow DOM bindings / contenteditable / GC), P2 п.3B (anim scheduling), P3 (Service Workers, Navigation API runtime, History API, IntersectionObserver triggers, IME, DevTools). | Изолированный новый крейт. P1/P2 не задеваются. |
| 2A | 🟡 **`[P3]` SOP / CORS preflight + mixed-content blocking + `<iframe sandbox>` flags enforcement.** CSP/HSTS/SRI уже parsed — подключаем к fetch path. **Security base реализован в ветке `network-security-base`:** `Origin` tuple (HTML LS §7.5), `classify_subresource_request` (W3C Mixed Content + Fetch §3.2.7 destinations), `SandboxFlags` u32-bitset + `parse_sandbox_value` (HTML LS §7.6.5, все 14 keyword-ов) — все три как pure-classifiers без enforcement в HttpClient. **Mixed-content enforcement реализован в ветке `mixed-content-enforcement`:** `MixedContentPolicy { top_level, mode: Disabled|SpecDefault|Strict }` + builder `HttpClient::with_mixed_content_policy` + `fetch_subresource(url, destination)`; classify в `fetch_with_redirect` ПОСЛЕ HSTS upgrade и ДО RequestFilter/RequestStarted, эмит `RequestBlocked { reason: "mixed-content: blockable|optionally-blockable" }`, per redirect-hop. `fetch` (top-level navigation) намеренно policy не enforces. **CORS preflight classifier+cache реализован в ветке `cors-preflight`:** `lumen-network::cors` — `is_cors_safelisted_method`, `is_forbidden_request_header`, `is_cors_safelisted_request_header`, `needs_preflight`, `unsafe_request_header_names`, `build_preflight_headers`, `evaluate_preflight_response` → `PreflightResult { allowed_methods, allowed_headers, allow_credentials, max_age_seconds }`, `check_cors_response_headers` (actual response), `PreflightCache` (thread-safe `(requestor, target, credentials_mode)` ключ, TTL=`max_age`, `allows_at` shortcut), `CredentialsMode { Omit, SameOrigin, Include }`, `CorsError` (8 кейсов). Pure-логика без HttpClient-интеграции. Остаётся: **CORS preflight enforcement в HttpClient** (отправка OPTIONS перед non-simple cross-origin, cache lookup, actual-response validation через `check_cors_response_headers`); sandbox-application в DOM-загрузчике shell-я. | Без enforcement браузер нельзя выпускать в публичную сеть. | Только `lumen-network` + shell. |
| 2C | **`[P3]` Tab session export / import** (§12.7) — сериализация в snapshot-формат `lumen-storage`. | UX-фича, экономит много боли пользователя. | Только `lumen-storage` + shell. |
| 3A | 🟡 **`[P3]` DPR + scroll в shell**. **DPR/scale_factor реализован в ветке `shell-dpr-support`**: `Renderer.scale_factor: f64` + `set_scale_factor` для `ScaleFactorChanged`, viewport uniform делится на DPR, 1 CSS px = `scale_factor` device px. Осталось: scroll-state в shell + scroll-to-match для find + relayout-on-resize (передавать реальный `inner_size` в layout вместо hardcoded 1024×720 — требует переноса layout-вызова в `Lumen::resumed()` после создания окна). | Реальные страницы на 4K выглядят корректно (DPR ✅). Layout-viewport — отдельным шагом. | Только `lumen-shell` + узкая правка `lumen-paint::renderer`. |
| 3B | **`[P3]` HTML event loop integration в Lumen-loop** (`run_idle_callbacks` в about_to_wait после step → Idle, правильный ordering rendering steps stage `style → layout → paint`, `scheduler.postTask`, реальные triggers observers, reload через queue_task с Rc<RefCell<Lumen>>). 🟡 Framework + winit-integration + task source priorities + requestIdleCallback готовы. | P1 / P2 видят правильный rAF tick — им ничего делать не надо. | Только `lumen-shell::runtime`. |
| 4A | **`[P3]` JS↔DOM bindings** поверх `lumen-js` от 1B. Использует уже готовые DOM API из `lumen-dom` (без правок DOM, кроме wrapper hooks по согласованию с P1). | Любая JS-driven динамика; Forms UI; Web Animations API surface. | Можно стартовать как только 1B готов. |
| 4B | **`[P1+P3]` Streaming pipeline shell-side** — window-first init, HTML fetch в фоновом потоке (chunks → main thread через channel), параллельный fetch subresources (thread pool / async), до прихода CSS — UA stylesheet, layout/paint reruns on dirty (relayout поддерева, throttle ~60 Гц). Зависит от P1 п.3B (push-tokenizer). | Первый кадр на Habr перестаёт ждать 5-15 сек. | До готовности P1 push-tokenizer-а P3 продолжает blocking-режим. |
| 5A | **`[P3]` HTTP/2** поверх своего rustls-based транспорта. | Современный latency на ресурсо-богатых страницах. | Только `lumen-network`. |
| 5B | **`[P3]` HTTP Range requests + `<video>` seek**. RFC 7233 single-range (closed / open-ended) + **suffix `bytes=-N`** + **If-Range** реализованы (`fetch_range(url, RangeSpec, Option<RangeValidator>)`). Осталось: multi-range / multipart/byteranges-парсер для одновременных запросов нескольких диапазонов (нужен `<video>` seek по таблице сегментов), интеграция в shell для resume downloads. | `<video>` seek (требует multi-range), PDF page-load, resume downloads (готов к интеграции). | Только `lumen-network`. |
| 5C | **`[P3]` DevTools / CDP минимум** (DOM tree + computed styles + network log с фильтрами). Стандарт CDP — WebSocket-сервер, совместим с puppeteer/playwright. | Без DevTools debug собственного движка катастрофа — экономия времени всем трём. | Можно стартовать после JS engine (1B). |
| 6+ | **`[P3]` `lumen-knowledge` finishing** — §12.4 поиск по открытым вкладкам, omnibox-префиксы `@history` / `@notes` / `@tabs` / `@read-later`, Porter-stemmer для русского · **`[P3]` Profiles + шифрование** (§9.3, XChaCha20-Poly1305 + Argon2id) · **`[P3]` Focus mode** (§12.6) · **`[P3]` Кастомизация UI** (§12.10) · **`[P3]` IME composition events** (Phase 2) · **`[P3]` WebSockets / SSE / Fetch API runtime** (Phase 3) · **`[P3]` OCSP stapling + CT log + invalid cert UI** · **`[P3]` Back/forward cache + Navigation/History API runtime** (Phase 3) · **`[P3]` Service Workers** (Phase 3, fetch interception backend + JS worker context + lifecycle) · **`[P3]` IndexedDB** (Phase 3, storage backend поверх SQLite + JS API surface) · **`[P3]` Spell check** через provisional `hunspell-rs` за `SpellChecker` от Sprint 0 (Phase 3) · **`[P3]` Permission prompt UI + Download UI** · **`[P3]` Site isolation** (process per origin) + **GPU process / sandbox** (Phase 3) · **`[P3]` V8 переход** через `rusty_v8` (Phase 3+) · **`[P3]` `lumen-ai` крейт** (§12.5, embedding + RAG + Ollama HTTP, Phase 3+) · **`[P3]` Семантические закладки** (§12.8). | См. fundamentals ниже. | Параллельны до точек координации с P1 (Shadow DOM bindings, GC integration). |

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
| `lumen-core::ext` | P3 | P1/P2 — добавляют trait если им нужно (sole-author commit, post-factum review) |
| `lumen-paint::display_list` (`DisplayCommand` enum) | P2 | P1 — добавляет варианты для новых layout-фич (например, для Grid) — P2 ревьюит |
| `samples/page.html`, snapshot tests | Кто меняет — тот и трогает | — |

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

---

### Browser fundamentals (справочный список, с учётом новой раскладки)

Список упущений, обнаруженных при сравнении с реальными движками 2026-05-14. Все маркеры обновлены под P1-P3. Каждый пункт **критичен для функциональности** при достижении соответствующей фазы — без них браузер не работает как браузер, а не Phase-0-демо.

#### Phase 1 (Reader) — добавить к существующему scope

- **`[P3]` HTML event loop + microtasks + rendering steps + observers.** 🟡 **Framework + integration в winit-loop + task source priorities + requestIdleCallback готовы** (`lumen-shell::runtime` + Lumen-handlers — см. [SUBSYSTEMS.md](SUBSYSTEMS.md) → `lumen-shell`). about_to_wait → step()×N (cap 256), Resized → deliver_observer_records(Resize), RedrawRequested → run_rendering_step(timestamp_ms) перед render(); `TaskQueue` обходит `TaskSource::PRIORITY_ORDER` на pop; `run_idle_callbacks(remaining_ms, now_ms)` доступен для шедулинга idle-работы. **Осталось:** reload через queue_task (требует Rc<RefCell<Lumen>>), интеграция `run_idle_callbacks` в Lumen-loop (в about_to_wait после step → Idle), правильный ordering rendering steps stage (style → layout → paint вместе с layout invalidation), `scheduler.postTask`, PerformanceObserver, реальные triggers observers (mutation/intersection events).
- **`[P1+P2]` Stacking contexts + правильный CSS Painting Order** (CSS 2.1 Appendix E). 7-уровневый порядок paint (background → border → block descendants → floats → non-positioned inline → positioned по z-index, рекурсивно). **P1** — модель stacking-ов и order computation в layout; **P2** — paint-side traversal в правильном порядке.
- **`[P2+P1]` Compositor thread + property trees.** Отдельные TransformTree / ScrollTree / EffectTree / ClipTree, копируются на compositor thread. Двухбуферная commit-модель. Off-main-thread scroll. **P2** — compositor pipeline / GPU primitives / layer tree; **P1 — готово (`PropertyTrees::build`, `Mat4` builders + multiply, ветка `property-trees-build`)** — построение четырёх деревьев из style/layout с учётом transform-origin.
- ✅ **`[P2]` Stacking-aware hit testing.** Реализовано в `lumen-paint::hit_test` (см. § «P2 2B» и [SUBSYSTEMS.md](SUBSYSTEMS.md) → `lumen-paint`).
- **`[P1]` Quirks mode vs standards mode — application в layout/cascade.** Detection реализован. Осталось: реально читать `Document.mode` и переключать legacy CSS-поведения.
- 🟡 **`[P3]` Same-Origin Policy enforcement + CORS preflight.** `Origin` tuple реализован (`lumen-network::Origin`, HTML LS §7.5 — scheme/host/port + same_origin + is_potentially_trustworthy). SOP checks при fetch / postMessage / storage / cookies — следующая задача (применить classifier в HttpClient). CORS OPTIONS preflight для non-simple requests; credentials mode (omit / same-origin / include) — отдельной веткой.
- 🟡 **`[P3]` Mixed-content blocking + `<iframe sandbox>`.** Classifier-ы реализованы (`lumen-network::classify_subresource_request` для blockable/optionally + `SandboxFlags`/`parse_sandbox_value` для всех 14 keyword-ов). Остаётся: enforcement в HttpClient (блочить blockable до TCP) + DOM-применение sandbox в shell.
- **`[P1+P3]` Preload scanner.** 🟡 **P1-часть готова** — `lumen_html_parser::preload_scanner::scan_preload_hints`. Осталось: **P3** — интеграция в shell-pipeline (когда запускать over chunks, как пробрасывать в HttpClient). Особенно полезно над streaming pipeline.

#### Phase 2 (Interactive) — без этого современный веб не функционален

- **`[P1+P3]` Shadow DOM + custom elements + `<template>` + `<slot>`.** **P1** — Shadow DOM cascade + composed tree + tree-builder; **P3** — JS bindings (`Element.attachShadow`, `customElements.define`) + lifecycle dispatch.
- **`[P1+P3]` Accessibility tree + platform bridges.** **P1** — построение accessibility tree из DOM/layout + ARIA semantics + focus model; **P3** — platform bridges (UIA / AT-SPI / NSAccessibility) + focus dispatch.
- **`[P1+P3]` Forms runtime.** **P1** — `ValidityState`, validation pseudo-classes, submission algorithm; **P3** — native pickers, autofill popup, validation tooltip UI.
- **`[P1+P2]` `<picture>` / `srcset` / `sizes` + `loading="lazy"`.** 🟡 **P1**: готовы parser + pickers + `pick_picture_source` + L4 nested `(not (...))` / `((...))` (см. § «P1 4A»). Осталось: IntersectionObserver event source для lazy. **P2** — image-side GPU upload + integration в shell.
- **`[P3]` IME composition events** (`compositionstart` / `update` / `end`, `KeyboardEvent.isComposing`). Интегрируется через winit IME API + DOM events.
- **`[P3]` Range requests — multi-range / suffix / If-Range.** Single-range уже реализован (`fetch_range`); осталось multi-range (multipart/byteranges), suffix `bytes=-N`, If-Range conditional, интеграция в shell под resume downloads и `<video>` seek. (Brotli — **готово**: `BrotliContentDecoder` за `ContentDecoder` в `lumen-network`.)
- **`[P3]` DevTools / Inspector минимум.** DOM tree view + computed styles panel + network log. Стандарт — Chrome DevTools Protocol (CDP) как WebSocket-сервер.
- **`[P1+P2]` `mix-blend-mode` / `background-blend-mode` / `isolation` / `backdrop-filter`.** 16 blend modes; `backdrop-filter` — отдельный pass blur на снапшоте под элементом.

#### Phase 3+ — без этого браузер не полнофункциональный

- **`[P3]` WebSockets (RFC 6455) + Server-Sent Events + Fetch API runtime.** WS: HTTP upgrade, frame-based binary protocol, ping/pong, permessage-deflate. SSE: `text/event-stream` + auto-reconnect. Fetch: Request / Response / Headers, ReadableStream body, AbortController.
- **`[P3]` HTTP auth — Basic + Digest готовы** (см. status). **Осталось:** Negotiate/NTLM, client certificates mTLS, UI-popup для credentials.
- **`[P3]` OCSP stapling + CT log enforcement + invalid cert UI.**
- **`[P3]` Safe Browsing — готово** (см. status). Отложено: 4-byte prefixes с full-hash callback, public-suffix list для безопасной обрезки host-suffixes ниже eTLD+1.
- **`[P3]` Back/forward cache (bfcache).** Снапшот DOM+JS heap для мгновенного back. Eligibility rules.
- **`[P3]` Navigation API + History API runtime.** `history.pushState` / `popstate`, `navigate` event.
- **`[P1+P2+P3]` Web Animations API runtime** поверх parsed `@keyframes` / transitions. **P1** — интерполяция; **P2** — compositor offload для transform / opacity; **P3** — animation timeline scheduling в rendering steps stage.
- **`[P1+P3]` `<contenteditable>` + Input Events Level 2 + Selection / Range API.** **P1** — DOM mutations + Selection / Range типы + `beforeinput` / `input`; **P3** — input dispatch (key + IME + drag-drop + paste), undo/redo stack в shell.
- **`[P3]` Service Worker runtime.** Fetch interception, push delivery, background sync, cache strategies. **P3** — и backend (fetch hook + storage), и JS worker context + lifecycle + `clients` API (бывший P4 объединён).
- **`[P3]` Spell check** через **provisional `hunspell-rs`** за `SpellChecker` от Sprint 0. Squiggly underline в render, context menu suggestions. Русский словарь — часть «русский first-class».
- **`[P2]` Variable fonts axes runtime.** `font-variation-settings`, interpolation по wght / wdth / slnt axes.
- **`[P2]` Color management + Display P3 / Rec2020 / ICC profiles.** Для `<canvas>` / `<img>` / CSS `color()` функций (CSS Color L4).
- **`[P1+P2+P3]` Print pipeline runtime.** **P1** — pagination algorithm; **P2** — PDF rendering из display list; **P3** — print preview UI.
- **`[P1+P3]` GC integration JS ↔ DOM (cycle collector).** **P1** — DOM-side wrapper hooks + lifecycle для трекинга cross-references; **P3** — JS engine integration + cycle collector algorithm + рабочий API на стыке.
- **`[P3]` Permission prompt UI + Download UI.**
- **`[P3]` GPU process / sandbox.** Реальный browser-grade sandbox: seccomp (Linux), AppContainer (Windows), App Sandbox (macOS), GPU процесс отдельно от renderer-а.

### Не приоритет, держим в голове

- **`[P2]`** Variable fonts parsers (fvar/gvar/avar/HVAR/VVAR/MVAR) в `lumen-font` — реализовано. Осталось runtime: применение deltas к outline в rasterizer-е (IUP), interpolation по wght/wdth/slnt axes, integration с CSS `font-variation-settings`.
- **`[P2]`** GSUB/GPOS shaping (для арабского, индийского, тайского). Текущая позиция — добавим как exception #5 (rustybuzz) или сами для базовых случаев.
- **`[P1]`** ADR-инфраструктура (`docs/decisions/`) — формализация decisions log. Любой может взять, но если все доменные программисты заняты крупным — это лёгкая «filler» задача с минимальными правками кода.
- **`[P3]`** StorageBackend trait: добавить origin partitioning параметр (`(origin, top_level_site)`) ДО первой реализации, чтобы не переделывать.
- Composite glyphs с ARGS_ARE_XY_VALUES=0 (point alignment) — реализовано — см. `git log --oneline | grep composite-point-align`.
- CSS4 pseudo-class `:has(...)` — реализовано — см. `git log --oneline | grep css-has-pseudo`.

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
- `FormData`, `Blob`, `File`
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

### 9.5 Anti-fingerprinting

- **Canvas randomization** — Canvas.getImageData возвращает данные с микро-шумом (как в Brave). Per-session seed.
- **WebGL renderer / vendor strings** — обобщённые («Generic GPU», «WebKit»).
- **AudioContext fingerprint** — мизерный шум.
- **Fonts enumeration** — белый список из системных шрифтов, без эксклюзивов.
- **Timezone** — опция «использовать UTC».
- **Screen resolution** — опция округления до 100px.
- **Hardware concurrency** — фиксируем на 2 или 4.

Три пресета:
- **Standard** — total cookie protection, adblock, strip URL params. Сайты работают.
- **Strict** — + fingerprinting protection, JS-блокировка на сомнительных доменах.
- **Tor-mode** — + через Tor, фиксированный fingerprint, никаких persistent данных.

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

CSS `hyphens: auto` с русскими правилами переноса. Откладываем до Phase 2 — не блокирует чтение, улучшает вёрстку. **Crate:** `hyphenation` (TeX-словари для русского доступны).

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

### 12.12 Где это всё трогает архитектуру

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

---

## 15. Тестирование

### 12.1 Уровни

1. **Unit-тесты** для каждого crate (`cargo test`).
2. **Парсер-тесты:**
   - `html5lib-tests` для HTML parser.
   - WPT-style тесты для CSS parser.
3. ✅ **Render snapshot tests** — рендерим страницу, сравниваем display list (не пиксели, так стабильнее). Реализовано: `serialize_display_list` + 6 golden-файлов в `lumen-paint/tests/snapshots/`. `UPDATE_SNAPSHOTS=1` для регенерации.
4. **Pixel snapshot tests** — для финальной картинки, с допуском.
5. **Web Platform Tests** — берём подмножество (DOM, CSS, fetch). Цель: 60% pass к v1.0.
6. **Integration tests** — запуск браузера, тест UI через `egui`-test-harness или внешний driver.
7. **Fuzzing** в CI.
8. **Top 1000 sites test** — на каждом релизе автоматический прогон, скриншоты, сравнение с Chromium как baseline.

### 12.2 CI

GitHub Actions: Linux/macOS/Windows, debug+release, `cargo test` + `cargo clippy -- -D warnings` + `cargo deny` + fuzzing 10 минут на PR.

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
- **Цель:** открыть простую текстовую статью без стилей. Доказательство концепции.

### Фаза 1 — v0.1 «Reader» (9 месяцев от старта)
- **Базовая пригодность shell** — без этого «открыть Habr-статью» невозможно как демо:
  - **Font fallback / matcher.** Рендерер сейчас всегда `Inter Regular` — любая страница с эмодзи / CJK / `font-family: Roboto` падает в `?`-глифы. Минимум: системный font-loader (Win32 GDI / fontconfig / CoreText — без сторонних crate-ов), cascade «Inter → системный по unicode-блоку». Парсер `font-family` уже есть, не используется в paint.
  - **HiDPI / DPR-awareness.** 🟡 paint-side: `Renderer` теперь хранит `scale_factor` и делит viewport uniform на него (1 CSS px = `scale_factor` device px на 4K). Layout-side: viewport остаётся hardcoded 1024×720; relayout-on-resize + передача реального `inner_size` в layout — отдельная задача (structural refactor pipeline).
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
  - **Quirks mode vs standards mode** (`[P1]`) — detection реализован (DocumentMode enum + quirks_mode::detect_document_mode по HTML5 §13.2.5.1); application в layout/cascade — отдельная задача. Без application половина legacy-страниц всё ещё рендерится как standards.
  - **Same-Origin Policy enforcement + CORS preflight** (`[P3]`) — SOP checks при fetch/postMessage/storage; OPTIONS preflight для non-simple requests.
  - **Mixed-content blocking + `<iframe sandbox>`** (`[P3]`) — HTTPS не грузит HTTP-script; sandbox flags.
  - **Preload scanner** (`[P1+P4]`) — отдельный pre-parser стартует fetch до DOM construction. Особенно важно над streaming pipeline. P1 — отдельный mode tokenizer-а; P4 — shell оркестрация.
- **Цель:** ежедневный браузер для чтения статей.

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
  - **`<picture>` / `srcset` / `sizes` + `loading="lazy"`** (`[P1+P2]`) — viewport+DPR-aware resource selection. P1 — `srcset` parser + density-picker + `sizes` parser + width-picker готовы (`lumen-html-parser::srcset`); осталось `<picture>`/`<source>` media-query selection + IntersectionObserver event source; P2 — image GPU upload.
  - **IME composition events** (`[P4]`) — без них японский / китайский / корейский ввод сломан.
  - **Connection pooling + keep-alive + Brotli + Range requests** (`[P3]`, ✅ keep-alive + Brotli + single-range; ⬜ multi-range / suffix / If-Range) — без keep-alive реальный сайт = 50× TCP handshakes.
  - **Find in page (Ctrl+F)** (`[P4]`).
  - **DevTools / Inspector минимум через CDP** (`[P4]`) — DOM tree + computed styles + network log. Без этого debug собственного движка невозможен.
  - **`mix-blend-mode` / `backdrop-filter` / `isolation`** (`[P1+P2]`) — нужны isolation groups в compositor pipeline. P1 — parsing + stacking model; P2 — paint pipeline + isolation groups.
- **Цель:** публичная альфа, форумы и простые SPA, в Lumen начинают **жить** долго.

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
