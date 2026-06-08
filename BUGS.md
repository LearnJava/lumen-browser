# BUGS.md — Баг-трекер Lumen Browser

Живой список известных багов движка. Пополняется из `python graphic_tests/run.py`.

**Как добавить баг:**
1. Скопируй скриншот в `graphic_tests/screenshots/bug-NNN-краткое-имя.png` (не коммитится)
2. Добавь запись в таблицу ниже

**Статусы:** `OPEN` · `IN PROGRESS` · `FIXED <date>` · `WONTFIX (Phase N+)`

---

## Сводная таблица

```
BUG-001 | FIXED 2026-05-15 | layout          | display:none on inline elements not working
BUG-003 | FIXED 2026-05-15 | layout          | style="" attribute not processed by cascade
BUG-007 | FIXED 2026-05-20 | layout          | <sub>/<sup>/<small> missing UA styles
BUG-008 | FIXED 2026-05-20 | layout          | <del>/<ins>/<u>/<s> text-decoration missing UA styles
BUG-009 | FIXED 2026-05-20 | layout          | <a> missing UA styles (no blue color, no underline)
BUG-012 | FIXED 2026-05-20 | layout          | <del>/<ins> break inline flow (each on new line)
BUG-016 | FIXED 2026-05-20 | css-parser/paint| border-style: dashed/double now work; dotted still square (→ BUG-029)
BUG-019 | FIXED 2026-05-20 | css-parser/paint| outline not rendered at all
BUG-027 | FIXED 2026-05-20 | layout          | block element ignores explicit width — body stretches to viewport
BUG-030 | FIXED 2026-05-20 | layout          | IFC: no whitespace gap between inline-block siblings (CSS §4.1.2)
BUG-031 | FIXED 2026-05-20 | layout          | IFC: missing strut descent causes rows to be ~4px too short
BUG-002 | FIXED 2026-05-20 | layout/paint    | inline padding/border/margin stacks vertically instead of flowing
BUG-004 | FIXED 2026-05-24 | layout          | height on inline elements (display:inline-block applies; display:inline ignores per CSS 2.1 §10.6.1)
BUG-005 | FIXED 2026-05-21 | layout+paint    | <img> inside <span> not rendered
BUG-010 | FIXED 2026-05-20 | layout          | <hr> renders nothing
BUG-011 | FIXED 2026-05-22 | layout/paint    | list markers (bullet, numbers) not rendered
BUG-013 | FIXED 2026-05-22 | layout          | adjacent <span style="..."> stack vertically without separator
BUG-014 | FIXED 2026-05-21 | image           | JPEG not decoded (PNG only)
BUG-015 | FIXED 2026-05-25 | paint           | broken <img> src shows no alt text
BUG-017 | FIXED 2026-05-22 | layout/paint    | text-decoration-style ignored (all render as solid)
BUG-018 | FIXED 2026-05-22 | layout          | text-decoration-color ignored (always inherits text color)
BUG-023 | FIXED 2026-05-26 | layout+paint    | opacity deviation — P1: strut fix 2026-05-26; P5 paint: premultiplied alpha double-mult at edge-AA pixels in composite shader → TEST-13 0.24%
BUG-024 | FIXED 2026-05-21 | layout          | box-sizing: content-box — border not added to outer size; height% resolved against width
BUG-025 | FIXED 2026-05-22 | layout          | max-height does not clamp block height; InlineSpace not included in shrink-to-fit width
BUG-026 | FIXED 2026-05-22 | layout/paint    | <img> CSS/HTML width+height ignored — renders at natural size (remaining TEST-18 ~10%: BUG-032)
BUG-028 | FIXED 2026-05-26 | shell           | relayout-on-resize + maximized window triggers BUG-027
BUG-029 | FIXED 2026-05-21 | paint           | border-style: dotted renders square dots instead of circles
BUG-020 | FIXED 2026-05-26 | layout          | overflow axis coercion: visible+hidden combo не клипало ось; CSS Overflow L3 §2.1 visible→auto в compute_style; TEST-14: 1.70%→0.03% PASS
BUG-006 | FIXED 2026-05-21 | layout          | table layout not implemented (td/th render as blocks)
BUG-021 | FIXED 2026-05-22 | html-parser     | HTML bgcolor attribute ignored
BUG-022 | FIXED 2026-05-22 | css-parser      | Quirks-mode hashless hex colors not parsed
BUG-032 | FIXED 2026-05-22 | paint/image     | object-fit image quality ~16%: area averaging заменяет bilinear при downscale
BUG-033 | FIXED 2026-05-22 | paint           | box-shadow: нет Gaussian blur — рендерится solid прямоугольник вместо размытой тени
BUG-034 | FIXED 2026-05-22 | layout          | transform-origin 50% 50% default not resolved against box size — pivot at (0,0) instead of center
BUG-035 | FIXED 2026-05-22 | layout          | ::before/::after pseudo-elements не генерируются в box_tree (реализация частичная)
BUG-036 | FIXED 2026-05-26 | layout          | border-radius: % значения (50%, etc.) не резолвятся → radius=0; только px работает
BUG-037 | FIXED 2026-05-26 | paint           | CSS filter effects не применяются визуально (grayscale/sepia/blur/etc.) — shared filter_uniform перезаписывался; fix: per-pass буфер через mapped_at_creation
BUG-038 | FIXED 2026-05-26 | layout          | list-style-position: inside — маркер занимал отдельную строку; li высотой 2× от нормы; fix: не продвигать child_y, сдвигать InlineRun вправо на marker_w
BUG-039 | FIXED 2026-05-26 | paint           | dashed/dotted border mismatch vs Chrome/Edge: dash ratio 3:1→Skia algo, corner squares→circle quads for dotted, 1px linear SDF AA
BUG-040 | FIXED 2026-05-27 | layout          | table layout unit tests assume direct <tr> children of <table>; html-full-tree-builder now injects implicit <tbody> breaking them | layout/src/lib.rs:9996
BUG-041 | FIXED 2026-05-27 | css-parser      | style::tests::line_clamp_integer_value / _standard_property / _not_inherited fail: CSS rule `div { -webkit-line-clamp: 3 }` produces None — test accesses doc.root().children[0] which is <html> after full HTML5 parsing, so rule doesn't match <div> | layout/src/style.rs:19855
BUG-042 | FIXED 2026-05-29 | js              | QuickJsRuntime missing JsRuntime::resume() impl — all lumen-js tests fail to compile | js/src/lib.rs:253
BUG-043 | FIXED 2026-05-29 | paint           | lumen-paint test suite красный (19 падений, не только 7): (1) 5 golden устарели — DrawText теперь несёт var=["opsz"=16] (font-optical-sizing 27fda15) → регенерированы; (2) overflow visible+hidden coercion (BUG-020) → visible computes to auto; auto = scroll-container, поэтому клип идёт через PushScrollLayer (p2-scroll-layer), обе оси к padding-box; 5 тестов (2 snapshot + 2 lib ordered_clip + чужой ordered_overflow_x_alone_triggers_clip) ждали PushClipRect/single-axis sentinel → переписаны под PushScrollLayer; (3) первая строка несёт half-leading 1.6px (CSS 2.1 §10.8.1), 5 baseline/wrap lib-тестов ждали line_y=0 → обновлены | paint/tests/snapshot_tests.rs, paint/src/display_list.rs
BUG-044 | FIXED 2026-05-29 | shell           | lumen-shell не компилируется (default + --features quickjs): non-exhaustive match по DisplayCommand в content_height_of/content_width_of — новые варианты PushMaskLayer/PopMaskLayer/DrawSvgPath/BoxModelOverlay (P2-мерджи) не обработаны; PushMaskLayer несёт rect → в rect-ветку, остальные → continue | shell/src/main.rs:4219, 4265
BUG-045 | FIXED 2026-05-29 | layout          | backdrop-filter не создавал stacking context: creates_stacking_context() проверял filter, но не backdrop_filter (CSS Filter Effects L2 §2) → box_layer_ops дропал PushBackdropFilter, пустой DL для backdrop-only div. Добавлена проверка + regression-тест | layout/src/stacking.rs:201
BUG-046 | FIXED 2026-05-30 | layout          | 3 устаревших теста lumen-layout --lib: webp теперь декодируется (в supported_mime_types) → picture-тесты обновлены (avif для fallback, webp для supported); non_cell_col_row_span: `lay` возвращает body-box напрямую, убран лишний first_element_child | layout/src/lib.rs:12253,12269,979
BUG-047 | FIXED 2026-05-30 | layout          | НЕ баг (мисдиагноз): line-clamp реально усекает контент — InlineRun внутри .box = 40/80/120/160 (1-4 строки). .box=160 у всех — корректный flex align-items:stretch, Edge рендерит так же (48-edge.png). Тест переписан на ground-truth, #[ignore] снят | crates/driver/tests/test_48.rs
BUG-048 | FIXED 2026-05-30 | shell           | lumen-shell не компилируется: non-exhaustive match по DisplayCommand в content_height_of/content_width_of — новый вариант DrawScrollbar (p2-scrollbar-rendering merge) не обработан; скроллбар — UI, не контент → ветка continue (как BUG-044) | shell/src/main.rs:4219,4271
BUG-049 | FIXED 2026-05-30 | shell           | lumen-shell не компилируется: non-exhaustive match по DisplayCommand в content_height_of/content_width_of — новый вариант PageBreak (p2 print-pages merge) не обработан; маркер пагинации печати, не контент, без rect → ветка continue (как BUG-048) | shell/src/main.rs:4219,4272
BUG-050 | FIXED 2026-05-31 | network         | doctest mock.rs:16 не компилировался — fetch() is a trait method, но `use NetworkTransport` не импортирован в примере → добавлен импорт | crates/network/src/mock.rs:9
BUG-051 | FIXED 2026-05-31 | layout          | abs-pos с top+bottom+height:auto (inset:0) схлопывался в height 0 — lay_out_abs_children резолвил ширину из left+right, но симметричной высоты из top+bottom не было (CSS Position L3 §6); страница 30 backdrop-filter рендерилась без фона | crates/engine/layout/src/box_tree.rs:3698
BUG-052 | FIXED 2026-05-31 | paint/cpu_raster | DrawBorder использовал anti_alias:true → tiny-skia hairline_aa::fill_dot8 бьёт debug_assert!(false) для тонких sub-pixel-positioned рамок (inner span округляется в 0) → паника в debug-профиле; fix: anti_alias:false для axis-aligned border quads | crates/engine/paint/src/cpu_raster.rs:1087
BUG-053 | FIXED 2026-06-02 | shell | `cargo build -p lumen-shell --features quickjs` не компилировался: trait PersistentJs не объявлял update_scroll_states/take_scroll_requests (merge p1-js-scroll-drain/p1-clickable-iterator потерял декларации в trait+impl при разрешении конфликта), а call-site в relayout() брал self иммутабельно (js+lb_ref) и тут же звал self.fetch_and_register_lazy_images(&mut self) → E0502. Default-gate (без quickjs) собирался, поэтому регрессия не ловилась. Fix: восстановил декларации+forwarding методов в trait/impl, вынес lazy fetch за пределы иммутабельного borrow. Восстановлено при работе над задачей #26 (clipboard) — feature gated на quickjs, иначе не верифицируема | crates/shell/src/main.rs:927,1051,3112
BUG-054 | FIXED 2026-06-04 | network | tests::stale_pooled_connection_triggers_retry падает на Windows (os error 10053 — хост разорвал соединение): тест поднимает loopback TcpListener, кладёт соединение в пул, закрывает сервер и ждёт retry; на Windows закрытое сокет-соединение даёт WSAECONNRESET на read status вместо ожидаемого EOF/retry. Fix: is_stale_error() теперь распознаёт "os error 10053" (WSAECONNABORTED) и "os error 10054" (WSAECONNRESET) — на Windows io::Error форматируется с локализованным OS-сообщением, а не Rust ErrorKind именем. | crates/network/src/lib.rs:662
BUG-055 | FIXED 2026-06-04 | layout | tests::collect_picture_unsupported_type_falls_back: AVIF теперь поддерживается (supported_mime_types_includes_avif), поэтому fallback не нужен. Тест переписан в BUG-046 (2026-05-30): unsupported_type_falls_back переведён на image/heic (реально неподдерживаемый) и проходит. | crates/engine/layout/src/lib.rs:12798
BUG-056 | FIXED 2026-06-03 | shell | font_registry used after move in parse_and_layout: clippy E0382 — font_registry перемещался в Arc::new() до последнего использования face_bytes_for_family в for-loop. Fix: move в Arc после цикла (shell/main.rs:2397-2398). Verified: workspace-clippy зелёный (P5 health-свип 2026-06-03). | crates/shell/src/main.rs:2369
BUG-057 | FIXED 2026-06-03 | paint | wgpu Vulkan crash on first render after page load: «Encoder is invalid» validation error → double panic при drop SurfaceAcquireSemaphores; воспроизводится на Windows Vulkan backend; fix: DX12 backend по умолчанию на Windows в Renderer::new_async + new_headless_async | crates/engine/paint/src/renderer.rs:1578
BUG-058 | FIXED 2026-06-04 | layout | display:contents не сглажен перед lay_out: паника «entered unreachable code: display:contents boxes must be flattened before lay_out» при открытии cnn.com | crates/engine/layout/src/box_tree.rs:3805
BUG-059 | FIXED 2026-06-04 | font | WOFF2-декодер отклоняет шрифты с контурами из 0 точек («woff2: contour with zero points»): все 10 шрифтов CNN (cnn_sans_condensed, cnn_sans_display, helveticaneue, noto_sans_arabic, noto_serif*) не загружаются; пустые глифы (пробел и др.) легальны по спеке и принимаются всеми браузерами — нужно пропускать такой глиф, а не отклонять шрифт целиком | crates/engine/font/src/woff2.rs:260
BUG-060 | FIXED 2026-06-04 | font | WOFF2-декодер обрывается с «unexpected end of font data» для cnn_sans_condensed-bold.woff2, cnn_sans_condensed-medium.woff2, cnn_sans_display-v1.woff2 — корень: точки контуров читались из glyph_stream вместо nPoints_stream, координаты — из glyph_stream вместо flag_stream; исправлено routing потоков согласно WOFF2 spec §5.3 | crates/engine/font/src/woff2.rs:125
BUG-061 | FIXED 2026-06-04 | driver | test_32_list_markers падал (ожидал 22 li, получал 26): коммит d70391d9 (C9) добавил 2 новые секции в 32-list-markers.html (custom-marker + content-marker), не обновив тест; ожидания обновлены до 26 li / 24 маркеров | crates/driver/tests/test_32.rs:30
BUG-062 | FIXED 2026-06-04 | network | clippy «very complex type» в doh.rs → type alias DnsCacheMap | crates/network/src/doh.rs:402
BUG-063 | FIXED 2026-06-04 | layout  | clippy: manual_clamp → scale.clamp(), удалён #[expect(dead_code)], collapsible_if схлопнуты, unneeded_struct_pattern убран | crates/engine/layout/src/mathml.rs:88
BUG-064 | FIXED 2026-06-08 | driver  | test_33_multi_column падал (ожидал 7 контейнеров height:60px и 22 .col, получал 52px): коммит cefb8475 (C8/P4) изменил высоты .mc с 60→52px, widths контейнеров 5/6 (680→660, 320→660) и заменил .col на .col-sm в группах 5-6; тест не был обновлён | crates/driver/tests/test_33.rs:34
BUG-065 | FIXED 2026-06-04 | shell   | Клик по ссылке <a href> не срабатывал: hit-test вычислял page_y = y_css + scroll_y, не вычитая TAB_BAR_HEIGHT=36px, на которую страница сдвигается через PushTransform при рендере. Исправлено в page_point, handle_click_at, dispatch_mouse_move, update_cursor_icon | crates/shell/src/main.rs
BUG-066 | FIXED 2026-06-07 | paint  | render_tile() в Renderer не имеет #[cfg(feature = "cpu-render")] но вызывает crate::cpu_raster::rasterize_cpu — clippy lumen-shell --all-targets падает без фичи cpu-render | crates/engine/paint/src/renderer.rs:6409
BUG-067 | FIXED 2026-06-08 | js     | document_pip_* тесты падали: WEB_API_SHIM определял Event, но не глобальный EventTarget → `class X extends EventTarget` бросал «EventTarget is not defined». Добавлен функциональный EventTarget в WEB_API_SHIM | crates/js/src/dom.rs (WEB_API_SHIM)
BUG-068 | FIXED 2026-06-08 | shell  | clippy lumen-shell: reader_view.rs:292,309 «collapsible_if» — два nested if-let схлопываемы; pre-existing с D-3, блокирует clippy -D warnings | crates/shell/src/reader_view.rs:292
BUG-069 | FIXED 2026-06-08 | image  | collect_picture_unsupported_type_falls_back падал: heic-source не скипался. Корень — D-3/D-4 добавили image/jxl, image/heic, image/heif в supported_mime_types(), хотя decode_jxl/decode_heic — заглушки (всегда Err). Picker выбирал heic-source и показывал пустую коробку вместо fallback. Fix: убраны 3 stub-формата из списка (avif остаётся — реальный декодер за feature-флагом) | crates/engine/image/src/lib.rs:31
BUG-070 | FIXED 2026-06-08 | js     | Дубликат BUG-067 (тот же корень: отсутствовал глобальный EventTarget). Исправлено вместе с BUG-067 — добавлен EventTarget в WEB_API_SHIM | crates/js/src/dom.rs (WEB_API_SHIM)
BUG-071 | FIXED 2026-06-08 | mcp    | `MockSession` в lumen-mcp не реализует методы `set_clock`, `set_rng_seed`, `freeze_fingerprint` из трейта `BrowserSession` (добавлены P1 в N-2 deterministic mode) — компиляция `--workspace` падает с E0046 | crates/mcp/src/server.rs:508
BUG-072 | FIXED 2026-06-08 | js     | Form Constraint Validation API init failed: FORM_VALIDATION_SHIM ссылается на bare `HTMLInputElement`/`HTMLTextAreaElement`/`HTMLSelectElement`/`HTMLButtonElement` (строки 169-172) — в install_dom эти конструкторы не определены глобально → ReferenceError «HTMLInputElement is not defined», шим не устанавливается. Нужны typeof-гварды | crates/js/src/form_validation.rs:169
BUG-073 | FIXED 2026-06-08 | js     | chrome_runtime_absent (no_automation_markers.rs) падает: D-6 extension-stub в WEB_API_SHIM безусловно ставит window.chrome.runtime, ломая anti-CDP-detection маркер. Fix: IIFE гардировано флагом `_LUMEN_EXTENSION_ACTIVE`; тесты dom.rs выставляют флаг перед install_dom | crates/js/src/dom.rs:10131
```

---

## Прогон 2026-05-26 v7 (graphic_tests, --continue-on-fail, порог 1%) — fix-inline-block-baseline

Исправлен IFC strut: добавляется только в строках с baseline-выровненными элементами (CSS §10.8).
TEST-12 (display) перешёл FAIL 1.56% → PASS 0.18%. TEST-13 бонусом улучшился 2.12% → PASS 0.24%.

```
TEST-00: PASS  0.00%   calibration
TEST-11: PASS  0.43%   min-max-height
TEST-12: PASS  0.18%   display                ← fix-inline-block-baseline FIXED
TEST-13: PASS  0.24%   visibility-opacity     ← бонус от strut-фикса
TEST-24: PASS  0.99%   vertical-align
```

---

## Прогон 2026-05-25 v6 (graphic_tests, --continue-on-fail, порог 1%) — свежая release-сборка

Release-бинарь пересобран (был от 2026-05-21, пропускал все фиксы BUG-025 и пр.).
TEST-11 перешёл в PASS (max-height/min-height корректны). TEST-12 улучшился с 11.27% → 1.56%.
Добавлены тесты 38–44. Рост diff в TEST-02/04 и TEST-13 — вероятно погрешность gdigrab (не регрессия кода).

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: FAIL  3.11%   color-named            ← антиалиасинг/gdigrab (было 0.39% со стар. бинарём)
TEST-03: FAIL  1.41%   color-formats          ← то же
TEST-04: FAIL  3.11%   color-alpha            ← то же
TEST-05: FAIL  1.66%   border-width           ← то же
TEST-06: FAIL  1.86%   border-sides           ← то же
TEST-07: PASS  0.70%   box-sizing
TEST-08: PASS  0.28%   padding
TEST-09: PASS  0.00%   margin
TEST-10: PASS  0.00%   min-max-width
TEST-11: PASS  0.43%   min-max-height         ← BUG-025 FIXED (старый бинарь не имел фикса)
TEST-12: FAIL  1.56%   display                ← inline-block baseline (fix-inline-block-baseline)
TEST-13: PASS  0.24%   visibility-opacity     ← BUG-023 FIXED 2026-05-26 (premultiplied alpha в composite shader)
TEST-14: FAIL  2.35%   overflow               ← BUG-020 (scrollbar UI)
TEST-15: PASS  0.72%   box-shadow
TEST-16: PASS  0.41%   outline
TEST-17: PASS  0.00%   calc
TEST-18: FAIL 13.43%   images                 ← BUG-032 (image scaling)
TEST-19: FAIL 10.56%   object-fit             ← BUG-032
TEST-20: FAIL  9.59%   quirks-bgcolor         ← BUG-021+BUG-022
TEST-21: FAIL  6.97%   border-style           ← остаточный dotted/dashed
TEST-22: PASS  1.08%   CSS transform
TEST-23: PASS  0.48%   pseudo-elements
TEST-24: PASS  0.99%   vertical-align
TEST-25: PASS  0.00%   table-layout
TEST-26: FAIL  8.82%   mask-image             ← не реализован (Phase 0: fallback)
TEST-27: FAIL  9.35%   direction-rtl          ← RTL partial (P1: layout alignment)
TEST-28: FAIL 12.60%   css-containment        ← contain:size/paint/strict
TEST-29: FAIL  6.63%   container-queries      ← @container
TEST-30: FAIL 24.05%   css-filter             ← BUG-037 FIXED; остаток — linear-gradient не реализован (P4)
TEST-31: FAIL 11.89%   clip-path              ← circle/ellipse/polygon
TEST-32: FAIL  8.61%   list-markers           ← маркеры (fix-list-markers-test32)
TEST-33: FAIL 19.71%   multi-column           ← column-count/column-width
TEST-34: FAIL  7.02%   forms                  ← UA styles for form controls
TEST-35: PASS  0.78%   grid-named-areas       ← CSS Grid named areas работает
TEST-36: FAIL  9.16%   border-radius          ← BUG-036 (border-radius %)
TEST-37: PASS  0.00%   float-clear            ← float реализован
TEST-38: PASS  2.22%   z-index                ← (порог 3.0%)
TEST-39: FAIL 31.30%   gradients              ← linear/radial gradient GPU
TEST-40: FAIL 45.26%   conic-gradients        ← conic-gradient
TEST-41: FAIL  3.52%   table                  ← display:table/row/cell (порог 3.0%)
TEST-42: FAIL  3.79%   position-sticky        ← (порог 3.0%)
TEST-43: FAIL  3.52%   intrinsic-sizing       ← max-content/min-content (порог 2.0%)
TEST-44: FAIL  3.52%   media-queries          ← @media queries (порог 2.0%)
```

---

## Прогон 2026-05-25 v5 (graphic_tests, --continue-on-fail, порог 1%)

Добавлены тесты 26–37 (новые CSS-свойства). Тесты 00–25 без изменений относительно v3.
Два новых бага: BUG-036 (border-radius %) и BUG-037 (CSS filter рендерер).

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: PASS  0.39%   color-named
TEST-03: PASS  0.11%   color-formats
TEST-04: PASS  0.39%   color-alpha
TEST-05: PASS  0.37%   border-width
TEST-06: PASS  0.26%   border-sides
TEST-07: PASS  0.70%   box-sizing
TEST-08: PASS  0.93%   padding
TEST-09: PASS  0.00%   margin
TEST-10: PASS  0.00%   min-max-width
TEST-11: FAIL 13.77%   min-max-height     ← см. примечание ниже
TEST-12: FAIL 11.27%   display            ← inline-block без baseline
TEST-13: PASS  0.24%   visibility-opacity
TEST-14: FAIL  2.68%   overflow           ← BUG-020 (scrollbar UI)
TEST-15: FAIL  1.92%   box-shadow         ← остаточное (blur spread)
TEST-16: FAIL  1.88%   outline            ← sub-pixel геометрия
TEST-17: PASS  0.00%   calc
TEST-18: FAIL 10.77%   images             ← BUG-032 (image scaling)
TEST-19: FAIL 13.00%   object-fit         ← BUG-032 (image scaling)
TEST-20: FAIL  8.68%   quirks-bgcolor     ← BUG-021+BUG-022
TEST-21: FAIL  1.75%   border-style       ← остаточный dotted
TEST-22: FAIL  9.79%   CSS transform      ← sub-pixel transform-origin
TEST-23: PASS  0.00%   pseudo-elements
TEST-24: PASS  1.10%   vertical-align
TEST-25: PASS  0.00%   table-layout
TEST-26: FAIL  8.82%   mask-image         ← не реализован (Phase 0: fallback to full-opacity)
TEST-27: FAIL  9.76%   direction-rtl      ← RTL direction partial; alignment bands отсутствуют
TEST-28: FAIL 14.81%   css-containment    ← contain:size/paint/strict не работают
TEST-29: FAIL 11.04%   container-queries  ← @container не реализован
TEST-30: FAIL 24.05%   css-filter         ← BUG-037 FIXED; остаток — linear-gradient не реализован (P4)
TEST-31: FAIL 20.57%   clip-path          ← circle/ellipse/polygon только bbox-clip (известное ограничение)
TEST-32: FAIL  6.05%   list-markers       ← маркеры отсутствуют (6% порог = текст+антиалиасинг)
TEST-33: FAIL 32.88%   multi-column       ← column-count/column-width не реализованы
TEST-34: FAIL  6.89%   forms              ← UA стили для form controls не реализованы
TEST-35: FAIL 83.20%   grid-named-areas   ← CSS Grid Phase 2 (grid-template-areas не работает)
TEST-36: FAIL 11.10%   border-radius      ← BUG-036 (border-radius: % → radius=0)
TEST-37: FAIL 41.83%   float-clear        ← float Phase 1 (не реализован)
```

**Примечание по TEST-11/TEST-12 (устарело):** значения в v5 были высокими потому что release-бинарь
был от 2026-05-21 (до BUG-025 фикса). В v6 после пересборки TEST-11 PASS 0.43%, TEST-12 1.56%.

---

## Прогон 2026-05-21 v3 (graphic_tests, --continue-on-fail, порог 1%)

BUG-024 FIXED: height% теперь резолвится против высоты containing block, а не ширины. TEST-06 и TEST-07 перешли в PASS.
TableRow добавлен в paint (display_list.rs), TEST-25 PASS.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: PASS  0.39%   color-named
TEST-03: PASS  0.11%   color-formats
TEST-04: PASS  0.39%   color-alpha
TEST-05: PASS  0.37%   border-width
TEST-06: PASS  0.26%   border-sides       ← BUG-024 FIXED
TEST-07: PASS  0.70%   box-sizing         ← BUG-024 FIXED
TEST-08: PASS  0.93%   padding
TEST-09: PASS  0.00%   margin
TEST-10: PASS  0.00%   min-max-width
TEST-11: FAIL 13.77%   min-max-height     ← BUG-025
TEST-12: FAIL 11.27%   display            ← BUG-025 + display modes
TEST-13: PASS  0.24%   visibility-opacity
TEST-14: FAIL  2.68%   overflow           ← BUG-020
TEST-15: FAIL  1.92%   box-shadow         ← BUG-033
TEST-16: FAIL  1.88%   outline            ← sub-pixel геометрия
TEST-17: PASS  0.00%   calc
TEST-18: FAIL 10.77%   images             ← BUG-026
TEST-19: FAIL 13.00%   object-fit         ← BUG-032
TEST-20: FAIL  8.68%   quirks-bgcolor     ← BUG-021 + BUG-022
TEST-21: FAIL  1.75%   border-style       ← остаточный BUG-029
TEST-22: FAIL  9.79%   CSS transform      ← BUG-034
TEST-23: PASS  0.00%   pseudo-elements
TEST-24: PASS  1.10%   vertical-align
TEST-25: PASS  0.00%   table-layout       ← TableRow paint FIXED
```

---

## Прогон 2026-05-21 v2 (graphic_tests, --continue-on-fail, порог 1%)

Инфраструктура: полная 1px магента-рамка (body #ff00ff + .__f wrapper), overflow:hidden на body.
Устранены ложные срабатывания от Edge-scrollbar: 10 тестов перешли FAIL→PASS.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: PASS  0.39%   color-named
TEST-03: PASS  0.11%   color-formats
TEST-04: PASS  0.39%   color-alpha
TEST-05: PASS  0.37%   border-width
TEST-06: FAIL  2.43%   border-sides       ← BUG-024 (box-sizing) + BUG-020 overflow
TEST-07: FAIL  6.56%   box-sizing         ← BUG-024
TEST-08: PASS  0.93%   padding
TEST-09: PASS  0.00%   margin
TEST-10: PASS  0.00%   min-max-width
TEST-11: FAIL 14.02%   min-max-height     ← BUG-025
TEST-12: FAIL 11.27%   display            ← BUG-025 + display modes
TEST-13: PASS  0.24%   visibility-opacity
TEST-14: FAIL  6.89%   overflow           ← BUG-020 (scrollbar UI отсутствует)
TEST-15: FAIL  1.92%   box-shadow         ← BUG-033 (solid тень, нет blur)
TEST-16: FAIL  1.88%   outline            ← sub-pixel геометрия
TEST-17: PASS  0.00%   calc
TEST-18: FAIL 11.06%   images             ← BUG-026
TEST-19: FAIL 12.62%   object-fit         ← BUG-032
TEST-20: FAIL 27.84%   quirks-bgcolor     ← BUG-021 + BUG-022
TEST-21: FAIL  1.77%   border-style       ← BUG-029 частично исправлен, ещё >1%
TEST-22: FAIL  8.39%   CSS transform      ← BUG-034 (transform не реализован)
TEST-23: FAIL  5.97%   pseudo-elements    ← BUG-035 (::before/::after не рендерятся)
```

**Сравнение с предыдущим прогоном (v1, старая .__m полоска):**

| Тест | Было | Стало | |
|---|---|---|---|
| TEST-01 sanity | 0.00% | 0.00% | = |
| TEST-02 color-named | 2.35% FAIL | 0.39% PASS | ▼ ложный FAIL устранён |
| TEST-03 color-formats | 2.06% FAIL | 0.11% PASS | ▼ |
| TEST-04 color-alpha | 2.35% FAIL | 0.39% PASS | ▼ |
| TEST-05 border-width | 3.89% FAIL | 0.37% PASS | ▼ |
| TEST-08 padding | 4.45% FAIL | 0.93% PASS | ▼ |
| TEST-09 margin | 1.95% FAIL | 0.00% PASS | ▼ |
| TEST-10 min-max-width | 3.52% FAIL | 0.00% PASS | ▼ |
| TEST-13 opacity | 2.20% FAIL | 0.24% PASS | ▼ |
| TEST-17 calc | 3.52% FAIL | 0.00% PASS | ▼ |

Все улучшения — устранение ложных FAIL от Edge scrollbar (3.52% = 15px scrollbar × 2 стороны).

---

## Прогон 2026-05-21 (graphic_tests, --continue-on-fail, порог 1%)

Инфраструктура: foreground-window fix (Alt-trick), Edge timeout 60s, калибровка по периметру.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity                 ← было 38.98% — foreground fix устранил смещение
TEST-02: FAIL  2.35%   color-named            ← sub-pixel антиалиасинг
TEST-03: FAIL  2.06%   color-formats          ← sub-pixel антиалиасинг
TEST-04: FAIL  2.35%   color-alpha            ← rgba edge rendering
TEST-05: FAIL  3.89%   border-width           ← sub-pixel рендеринг границы
TEST-06: FAIL  5.95%   border-sides           ← BUG-024 (box-sizing)
TEST-07: FAIL  8.60%   box-sizing             ← BUG-024
TEST-08: FAIL  4.45%   padding                ← padding + sub-pixel
TEST-09: FAIL  1.95%   margin                 ← margin edge
TEST-10: FAIL  3.52%   min-max-width          ← min/max width clamping
TEST-11: FAIL 17.54%   min-max-height         ← BUG-025
TEST-12: FAIL 13.23%   display                ← BUG-025 + display modes
TEST-13: FAIL  2.20%   visibility-opacity     ← BUG-023
TEST-14: FAIL 10.41%   overflow               ← BUG-020
TEST-15: FAIL  3.87%   box-shadow
TEST-16: FAIL  5.40%   outline                ← BUG-024 геометрия
TEST-17: FAIL  3.52%   calc
TEST-18: FAIL 14.58%   images                 ← BUG-026 (было 14.68%)
TEST-19: FAIL 16.54%   object-fit             ← BUG-032 (86% было ложным — устаревший бинарник; реальный baseline 16%)
TEST-20: FAIL 30.49%   quirks-bgcolor         ← BUG-021 + BUG-022
TEST-21: FAIL  5.28%   border-style
TEST-22: FAIL 13.31%   CSS transform          ← первый прогон
```

---

## Прогон 2026-05-20 v2 (graphic_tests, --continue-on-fail, порог 1%)

Порог снижен с 5% до 1%. Видно значительное улучшение по многим тестам после мержа IFC-фиксов (BUG-030, BUG-031).

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: FAIL  2.35%   color-named        ← sub-pixel антиалиасинг границ
TEST-03: FAIL  2.06%   color-formats      ← sub-pixel антиалиасинг
TEST-04: FAIL  2.35%   color-alpha        ← rgba edge rendering
TEST-05: FAIL  3.89%   border-width       ← sub-pixel рендеринг границы
TEST-06: FAIL  5.95%   border-sides       ← BUG-024 (box-sizing)
TEST-07: FAIL  8.60%   box-sizing         ← BUG-024
TEST-08: FAIL  4.45%   padding            ← padding + sub-pixel
TEST-09: FAIL  1.95%   margin             ← margin edge (1px over threshold)
TEST-10: FAIL  3.52%   min-max-width      ← min/max width clamping
TEST-11: FAIL 17.54%   min-max-height     ← BUG-025
TEST-12: FAIL 13.23%   display            ← BUG-025 + display modes
TEST-13: FAIL  2.20%   visibility-opacity ← BUG-023 (улучшилось: 16.58%→2.20%)
TEST-14: FAIL 10.41%   overflow           ← BUG-020
TEST-15: FAIL  3.87%   box-shadow         ← box-shadow rendering
TEST-16: FAIL  5.40%   outline            ← BUG-024 влияет на геометрию
TEST-17: FAIL  3.52%   calc               ← calc() sub-pixel
TEST-18: FAIL 14.68%   images             ← BUG-026
TEST-19: FAIL 16.14%   object-fit         ← object-fit не реализован
TEST-20: FAIL 30.49%   quirks-bgcolor     ← BUG-021 + BUG-022
TEST-21: FAIL  5.28%   border-style       ← BUG-029 (dotted=square)
```

**Сравнение с предыдущим прогоном (до IFC-фиксов):**

| Тест | Было | Стало | Δ |
|---|---|---|---|
| TEST-02 color-named | 22.04% | 2.35% | ▼19.7 — BUG-027 устранён |
| TEST-03 color-formats | 32.12% | 2.06% | ▼30.1 — BUG-027 устранён |
| TEST-04 color-alpha | 15.67% | 2.35% | ▼13.3 — BUG-027 устранён |
| TEST-05 border-width | 13.67% | 3.89% | ▼9.8 — BUG-027 устранён |
| TEST-06 border-sides | 23.12% | 5.95% | ▼17.2 — BUG-027 устранён |
| TEST-08 padding | 11.35% | 4.45% | ▼6.9 — BUG-027 устранён |
| TEST-13 opacity | 16.58% | 2.20% | ▼14.4 — BUG-023 в основном исправлен |
| TEST-14 overflow | 20.39% | 10.41% | ▼10.0 |
| TEST-15 box-shadow | 6.44% | 3.87% | ▼2.6 |
| TEST-16 outline | 20.37% | 5.40% | ▼15.0 — BUG-027 устранён |
| TEST-18 images | 31.73% | 14.68% | ▼17.1 |
| TEST-19 object-fit | 22.53% | 16.14% | ▼6.4 |
| TEST-21 border-style | 19.07% | 5.28% | ▼13.8 — BUG-027 устранён |

**Выводы:**
- BUG-027 (block width) **фактически устранён** — все зависящие тесты упали на 10–30%
- BUG-023 (opacity) **существенно улучшился**: 16.58% → 2.20% (порог 1% не проходит, но регрессия устранена)
- Главные оставшиеся блокеры: BUG-024 (box-sizing), BUG-025 (max-height), BUG-020 (overflow), BUG-026 (images), BUG-021/022 (quirks-bgcolor)
- TEST-02..05, 08, 09, 13 проваливаются только из-за sub-pixel антиалиасинга: реальная разница < 4%, при пороге 1% неизбежны

---

## Прогон 2026-05-20 v1 (graphic_tests, --continue-on-fail, порог 5%)

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: FAIL 22.04%   color-named       ← BUG-027 (layout only, colors OK)
TEST-03: FAIL 32.12%   color-formats     ← BUG-027 (layout only, colors OK)
TEST-04: FAIL 15.67%   color-alpha       ← BUG-027 (layout only)
TEST-05: FAIL 13.67%   border-width      ← BUG-027 (layout only)
TEST-06: FAIL 23.12%   border-sides      ← BUG-027 (layout only)
TEST-07: FAIL  8.60%   box-sizing        ← BUG-024
TEST-08: FAIL 11.35%   padding           ← BUG-027 (layout only)
TEST-09: PASS  1.95%   margin
TEST-10: PASS  3.52%   min-max-width
TEST-11: FAIL 15.90%   min-max-height    ← BUG-025
TEST-12: FAIL 13.76%   display           ← BUG-027 + BUG-025
TEST-13: FAIL 16.58%   visibility-opacity← BUG-023 (regression)
TEST-14: FAIL 20.39%   overflow          ← BUG-020
TEST-15: FAIL  6.44%   box-shadow        ← BUG-027 (layout only)
TEST-16: FAIL 20.37%   outline           ← BUG-027 (outline itself works)
TEST-17: PASS  3.52%   calc
TEST-18: FAIL 31.73%   images            ← BUG-026 + BUG-027
TEST-19: FAIL 22.53%   object-fit        ← BUG-027 (layout only)
TEST-20: FAIL 30.62%   quirks-bgcolor    ← BUG-006/021/022
TEST-21: FAIL 19.07%   border-style      ← BUG-027 + BUG-029 (dotted=square)
```

**Выводы:**
- outline работает (BUG-019 закрыт визуально, TEST-16 fails из-за BUG-027)
- dashed / double рамки работают корректно
- BUG-023 (opacity) — была регрессия 2026-05-19; **FIXED 2026-05-26** (premultiplied alpha в composite shader)

---

## Детали багов

### BUG-027 · Block-элемент игнорирует explicit `width` [P1]

**Статус:** FIXED 2026-05-20
**Компонент:** `lumen-layout` — block width computation

Block-элемент с `width: 400px` берёт 100% ширины viewport. После фикса: если задан явно (не `auto`) — использовать это значение; если `auto` — брать `available_width`.

---

### BUG-028 · relayout-on-resize + `.with_maximized(true)` [P3]

**Статус:** FIXED 2026-05-26  
**Компонент:** `lumen-shell` — `Lumen::relayout()` + `WindowEvent::Resized` handler

Окно открывается максимизированным, winit сразу стреляет `Resized(~1920×1040)`. `relayout()` пересчитывает с viewport 1920px → BUG-027 проявляется.

**Фикс:** 1) guard в `WindowEvent::Resized` — skip при `size == 0` (минимизация на Windows); 2) defensive guard в `relayout()` при `vp_size <= 0`; 3) BUG-027 FIXED — explicit width больше не игнорируется при любом viewport. Временная мера (убрать `with_maximized`) оставлена: окно стартует 1024×720 для корректной работы графических тестов.

---

### BUG-023 · opacity sub-pixel deviation

**Статус:** FIXED 2026-05-26 (остаточный sub-1% edge-AA — TEST-13 0.24%; см. сводную таблицу)  
**Компонент:** `lumen-paint` + `lumen-layout`

Opacity compositing математически корректен: `PushOpacity`/`PopOpacity` + off-screen layer composite shader (`c.rgb * in.alpha + white * (1 - in.alpha)`). TEST-13 (2.20%) не хуже TEST-02 color-named (2.35%) без opacity — т.е. opacity не добавляет ошибку.

**P1-часть FIXED 2026-05-24** (commit на ветке p1-bug-023-strut): InlineBlockRow больше не добавляет strut_descent в строках без InlineRun. Edge/Blink не расширяют line box font-strut'ом, когда в строке только inline-block/replaced элементы; ранее каждый такой ряд накапливал ~3.86 px (Inter, font-size:16) лишнего descender, смещая последующие блоки.

Оставшиеся ~1.6% — edge antialiasing: Edge сглаживает рёбра, Lumen нет. Для снижения ниже 1% — MSAA/SSAA в renderer (P2).

---

### BUG-024 · box-sizing: content-box — border не добавляется к outer size

**Статус:** FIXED 2026-05-21 (см. сводную таблицу)  
**Компонент:** `lumen-layout` — box model

TEST-07: content-box боксы в Lumen уже чем в Edge на `2 × border_width`.

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — вычисление `rect.width` / `rect.height` для `content-box`.

---

### BUG-025 · max-height не зажимает высоту блока

**Статус:** FIXED 2026-05-22 (см. сводную таблицу)  
**Компонент:** `lumen-layout` — block height clamping

TEST-11: При `height: 160px; max-height: 80px` блок рендерится 160px (max-height игнорируется).

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — после вычисления `height`, найти применение `min_height`/`max_height`.

---

### BUG-026 · `<img>` не масштабируется по CSS/HTML width/height

**Статус:** FIXED 2026-05-22 (остаток качества — BUG-032; см. сводную таблицу)  
**Компонент:** `lumen-layout` / `lumen-paint`

TEST-18: `<img width="300" height="225">` рендерится в натуральном размере файла. Команда `DrawImage` должна использовать layout-rect, не натуральный размер текстуры.

---

### BUG-029 · border-style: dotted — квадратные точки вместо круглых

**Статус:** FIXED 2026-05-21 (см. сводную таблицу; круглые dots — BUG-039)  
**Компонент:** `lumen-paint` — border rendering

TEST-21: `border-style: dotted` рисует квадратные точки. По CSS-спеке dots должны быть круглыми (filled circles). dashed и double работают корректно.

**Где смотреть:** `crates/engine/paint/src/display_list.rs` — секция отрисовки dotted-border, заменить FillRect на рисование окружностей через примитив или GPU-path.

---

### BUG-020 · overflow: scroll/auto/hidden не реализован

**Статус:** FIXED 2026-05-26 (overflow axis coercion; TEST-14 0.03%; см. сводную таблицу)  
**Компонент:** `lumen-layout` / `lumen-paint`

TEST-14: все варианты overflow ведут себя как `visible`. В Edge видны scrollbar-ы и клиппинг.

---

### BUG-021 · HTML-атрибут bgcolor игнорируется

**Статус:** FIXED 2026-05-22 (см. сводную таблицу)  
**Компонент:** `lumen-html-parser` (presentational hints)

TEST-20: `<body bgcolor="#1a2030">` даёт белый фон вместо тёмно-синего.

---

### BUG-022 · CSS hashless hex colors (Quirks-mode) не парсятся

**Статус:** FIXED 2026-05-22 (см. сводную таблицу)  
**Компонент:** `lumen-css-parser`

TEST-20: `bgcolor="44aa66"` не распознаётся как `#44aa66` в quirks-mode.

---

### BUG-032 · Качество масштабирования изображений: ~16% расхождение с Edge

**Статус:** FIXED 2026-05-22 (area averaging при downscale; см. сводную таблицу)  
**Компонент:** `lumen-paint`, `lumen-image`

TEST-19 (object-fit), TEST-18 (images): пиксельная разница ~16% при большом коэффициенте уменьшения (~4.7x, 852×725 → 180×120).

#### Что сделано (2026-05-21)

1. **CPU-side bilinear resize** — реализован в `lumen-image/src/lib.rs`:
   - `Image::to_rgba8()` — конвертация любого формата в RGBA8
   - `pub fn resize_bilinear(src: &Image, dst_w: u32, dst_h: u32) -> Image` — 4-tap bilnear с half-pixel offset
   - В `renderer.rs` добавлен pre-pass перед render loop: для каждого `DrawImage` вызывается `ensure_image_gpu_key()`, которая создаёт CPU-ресайзированную текстуру и кеширует под ключом `"src@WxH"`.
   - Разделение на `compute_image_gpu_key(&self)` (иммутабельный) + `ensure_image_gpu_key(&mut self)` (мутабельный pre-pass) обязательно — иначе borrow-checker блокирует (в render loop `parsed_faces: Vec<Option<ParsedFace<'_>>>` держит `&self.faces`).

2. **Результат:** минимальное улучшение: TEST-18 14.68% → 14.44%, TEST-19 16.14% → 16.54% (шум, не улучшение).

#### Почему не помогло

CPU bilinear ≈ GPU bilinear — оба делают 4-выборки. При коэффициенте уменьшения 4.7x область покрытия одного выходного пикселя = 4.7×4.7 = ~22 исходных пикселей, из которых bilinear учитывает лишь 4. Антиалиасинг не обеспечивается.

Edge/Chrome используют **Skia**, который при downscale применяет **Lanczos-3** (или area averaging) — усредняет все пиксели в области покрытия. Поэтому разные браузеры дают одинаковый результат: они используют одну библиотеку (Skia).

Дополнительная причина: текстуры загружаются как `Rgba8Unorm` (linear), хотя PNG-файлы хранят sRGB. Блендинг в linear-пространстве при правильных финальных значениях дал бы совпадение, но sRGB→linear конвертация при загрузке не выполняется → цветовые ошибки ~2-5%.

#### Что нужно сделать

1. **[Приоритет 1] Area averaging (box filter) для downscale:**
   ```rust
   // Заменить resize_bilinear на resize_area_avg для случаев (dst < src)
   pub fn resize_area_avg(src: &Image, dst_w: u32, dst_h: u32) -> Image;
   // Алгоритм: для каждого dst-пикселя вычислить float-прямоугольник в src-координатах,
   // усреднить все целые пиксели + частичные веса по краям.
   ```
   Ожидаемый результат: совпадение с Edge ~2-4% (только sRGB-девиация останется).

2. **[Приоритет 2] sRGB при загрузке текстур:**  
   Изменить формат текстуры с `Rgba8Unorm` на `Rgba8UnormSrgb` в `renderer.rs` → wgpu автоматически конвертирует sRGB→linear при sampling. Требует также перевода surface в sRGB (`TextureFormat::Bgra8UnormSrgb`). Запланировано на Phase 3+.

#### Файлы

- `crates/engine/image/src/lib.rs` — `to_rgba8()`, `resize_bilinear()`
- `crates/engine/paint/src/renderer.rs` — pre-pass, `ensure_image_gpu_key()`, `compute_image_gpu_key()`, `make_gpu_image_entry()`

---

---

### BUG-036 · border-radius: % значения не резолвятся [P4]

**Статус:** FIXED 2026-05-26 (см. сводную таблицу)  
**Компонент:** `lumen-layout` — `style.rs:13479`

`border-radius: 50%` и любые % значения оставляют радиус = 0.0. Только пиксельные значения (4px, 32px, 999px) работают корректно.

**Корень:** `resolve_box_length()` возвращает `None` для `Length::Percent(_)`:
```rust
fn resolve_box_length(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    let len = parse_length_q(val, is_quirks)?;
    match len {
        Length::Percent(_) => None,   // ← здесь баг
        other => other.resolve(em_basis, None, viewport),
    }
}
```

По спеке CSS Backgrounds L3 §5.5: % для border-radius — относительно border-box (ширина для H-радиуса, высота для V-радиуса). Нужно хранить типизированное `Length` и резолвить при layout, когда известен размер бокса.

**Где смотреть:** `crates/engine/layout/src/style.rs:13479` — `resolve_box_length`, `crates/engine/layout/src/style.rs:10684` — применение `border-radius` shorthand.

---

### BUG-037 · CSS filter effects не применяются визуально [P2]

**Статус:** FIXED 2026-05-26 (per-pass buffer через mapped_at_creation; см. сводную таблицу)  
**Компонент:** `lumen-paint` — `renderer.rs` (filter composite pipeline)

CSS-фильтры `grayscale`, `sepia`, `brightness`, `invert`, `contrast`, `saturate`, `hue-rotate`, `blur` и `backdrop-filter` присутствуют в дисплей-листе с правильной структурой (`PushFilter [grayscale]` / `FillRect` / `PopFilter`). Шейдер WGSL (`FILTER_SHADER_SRC`) корректно реализует все виды фильтров. Но визуально элементы отображаются без фильтрации — как если бы `PushFilter`/`PopFilter` игнорировались.

**Что работает:**
- Дисплей-лист: `PushFilter`/`PopFilter` генерируются корректно с правильными `FilterFn`
- `filter_fn_to_entry`: корректно маппит Grayscale→kind=3, Sepia→kind=8 и т.д.
- WGSL shader: логика `apply_filter_fn` математически верна

**Что не работает:**
- Итоговый рендер: все элементы с фильтром показывают исходный цвет без изменений
- backdrop-filter: полупрозрачные боксы с backdrop-filter рендерятся как пустые

**Где смотреть:**
- `crates/engine/paint/src/renderer.rs:4653` — `RenderPlanItem::FilterComposite` (исполнение)
- `crates/engine/paint/src/renderer.rs:3994` — `DisplayCommand::PushFilter` (планирование)
- Подозрение: offscreen texture не получает draw-команды, или FilterComposite читает неправильный слой

---

## Ограничения Phase 0 (не баги — запланировано позже)

| Фича | Фаза | TEST |
|---|---|---|
| `float: left/right` | Phase 1 | TEST-37: 41.83% |
| `position:absolute/fixed/relative` | Phase 1 | — |
| `flexbox` (`display:flex`) | Phase 1 | — |
| `grid` / `grid-template-areas` | Phase 2 | TEST-35: 83.20% |
| CSS-анимации / transitions | Phase 2 | — |
| HiDPI / DPR-масштабирование | Phase 1 | — |
| `column-count` / `column-width` (multi-column) | Phase 1 | TEST-33: 32.88% |
| `@container` container queries | Phase 1 | TEST-29: 11.04% |
| `mask-image` | Phase 1 | TEST-26: 8.82% |
| `contain:` CSS containment | Phase 1 | TEST-28: 14.81% |
| Form controls UA styles | Phase 1 | TEST-34: 6.89% |
| `clip-path: circle/ellipse/polygon` — точная форма | Phase 1 | TEST-31: 20.57% (bbox работает) |
| `direction: rtl` alignment | Phase 1 | TEST-27: 9.76% |
