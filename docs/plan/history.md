> **Архив.** Хронология того, что уже реализовано. В текущей сессии читать не нужно — для этого есть `git log`. Актуальный статус задач — в `STATUS-PN.md`.

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

- **`srcdoc-sandbox-application`**: `apply_iframe_sandbox_gates` в shell — для `srcdoc`-iframe-ов парсит inline HTML и применяет gates (scripts/forms/navigation/popup) к внутреннему документу; для URL-based iframe-ов (Phase 0, не загружаются) логирует ограничения. 7 unit-тестов.

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


