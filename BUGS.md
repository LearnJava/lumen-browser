# BUGS.md — Баг-трекер Lumen Browser

Живой список известных багов движка. История прогонов — в `graphic_tests/results/*.json` (коммитируются).

**Как добавить баг:**
1. Создай файл `bugs/BUG-NNN-OPEN.md` (следующий номер по счёту, сейчас BUG-165)
2. Добавь строку в таблицу ниже со ссылкой на файл

**При изменении статуса:** переименуй файл (`BUG-NNN-OPEN.md` → `BUG-NNN-FIXED.md`) и обнови ссылку в таблице.

**Статусы:** `OPEN` · `IN PROGRESS` · `FIXED <date>` · `WONTFIX (Phase N+)`

---

## Список багов

| ID | Статус | Компонент | Описание |
|---|---|---|---|
| [BUG-001](bugs/BUG-001-FIXED.md) | FIXED 2026-05-15 | layout | display:none on inline elements not working |
| [BUG-002](bugs/BUG-002-FIXED.md) | FIXED 2026-05-20 | layout/paint | inline padding/border/margin stacks vertically instead of flowing |
| [BUG-003](bugs/BUG-003-FIXED.md) | FIXED 2026-05-15 | layout | style="" attribute not processed by cascade |
| [BUG-004](bugs/BUG-004-FIXED.md) | FIXED 2026-05-24 | layout | height on inline elements (display:inline-block applies; display:inline ignores per CSS 2.1 §10.6.1) |
| [BUG-005](bugs/BUG-005-FIXED.md) | FIXED 2026-05-21 | layout+paint | `<img>` inside `<span>` not rendered |
| [BUG-006](bugs/BUG-006-FIXED.md) | FIXED 2026-05-21 | layout | table layout not implemented (td/th render as blocks) |
| [BUG-007](bugs/BUG-007-FIXED.md) | FIXED 2026-05-20 | layout | `<sub>`/`<sup>`/`<small>` missing UA styles |
| [BUG-008](bugs/BUG-008-FIXED.md) | FIXED 2026-05-20 | layout | `<del>`/`<ins>`/`<u>`/`<s>` text-decoration missing UA styles |
| [BUG-009](bugs/BUG-009-FIXED.md) | FIXED 2026-05-20 | layout | `<a>` missing UA styles (no blue color, no underline) |
| [BUG-010](bugs/BUG-010-FIXED.md) | FIXED 2026-05-20 | layout | `<hr>` renders nothing |
| [BUG-011](bugs/BUG-011-FIXED.md) | FIXED 2026-05-22 | layout/paint | list markers (bullet, numbers) not rendered |
| [BUG-012](bugs/BUG-012-FIXED.md) | FIXED 2026-05-20 | layout | `<del>`/`<ins>` break inline flow (each on new line) |
| [BUG-013](bugs/BUG-013-FIXED.md) | FIXED 2026-05-22 | layout | adjacent `<span style="...">` stack vertically without separator |
| [BUG-014](bugs/BUG-014-FIXED.md) | FIXED 2026-05-21 | image | JPEG not decoded (PNG only) |
| [BUG-015](bugs/BUG-015-FIXED.md) | FIXED 2026-05-25 | paint | broken `<img>` src shows no alt text |
| [BUG-016](bugs/BUG-016-FIXED.md) | FIXED 2026-05-20 | css-parser/paint | border-style: dashed/double now work; dotted still square (→ BUG-029) |
| [BUG-017](bugs/BUG-017-FIXED.md) | FIXED 2026-05-22 | layout/paint | text-decoration-style ignored (all render as solid) |
| [BUG-018](bugs/BUG-018-FIXED.md) | FIXED 2026-05-22 | layout | text-decoration-color ignored (always inherits text color) |
| [BUG-019](bugs/BUG-019-FIXED.md) | FIXED 2026-05-20 | css-parser/paint | outline not rendered at all |
| [BUG-020](bugs/BUG-020-FIXED.md) | FIXED 2026-05-26 | layout | overflow axis coercion: visible+hidden combo не клипало ось; CSS Overflow L3 §2.1 visible→auto в compute_style; TEST-14: 1.70%→0.03% PASS |
| [BUG-021](bugs/BUG-021-FIXED.md) | FIXED 2026-05-22 | html-parser | HTML bgcolor attribute ignored |
| [BUG-022](bugs/BUG-022-FIXED.md) | FIXED 2026-05-22 | css-parser | Quirks-mode hashless hex colors not parsed |
| [BUG-023](bugs/BUG-023-FIXED.md) | FIXED 2026-05-26 | layout+paint | opacity deviation — P1: strut fix 2026-05-26; P5 paint: premultiplied alpha double-mult at edge-AA pixels in composite shader → TEST-13 0.24% |
| [BUG-024](bugs/BUG-024-FIXED.md) | FIXED 2026-05-21 | layout | box-sizing: content-box — border not added to outer size; height% resolved against width |
| [BUG-025](bugs/BUG-025-FIXED.md) | FIXED 2026-05-22 | layout | max-height does not clamp block height; InlineSpace not included in shrink-to-fit width |
| [BUG-026](bugs/BUG-026-FIXED.md) | FIXED 2026-05-22 | layout/paint | `<img>` CSS/HTML width+height ignored — renders at natural size (remaining TEST-18 ~10%: BUG-032) |
| [BUG-027](bugs/BUG-027-FIXED.md) | FIXED 2026-05-20 | layout | block element ignores explicit width — body stretches to viewport |
| [BUG-028](bugs/BUG-028-FIXED.md) | FIXED 2026-05-26 | shell | relayout-on-resize + maximized window triggers BUG-027 |
| [BUG-029](bugs/BUG-029-FIXED.md) | FIXED 2026-05-21 | paint | border-style: dotted renders square dots instead of circles |
| [BUG-030](bugs/BUG-030-FIXED.md) | FIXED 2026-05-20 | layout | IFC: no whitespace gap between inline-block siblings (CSS §4.1.2) |
| [BUG-031](bugs/BUG-031-FIXED.md) | FIXED 2026-05-20 | layout | IFC: missing strut descent causes rows to be ~4px too short |
| [BUG-032](bugs/BUG-032-FIXED.md) | FIXED 2026-05-22 | paint/image | object-fit image quality ~16%: area averaging заменяет bilinear при downscale |
| [BUG-033](bugs/BUG-033-FIXED.md) | FIXED 2026-05-22 | paint | box-shadow: нет Gaussian blur — рендерится solid прямоугольник вместо размытой тени |
| [BUG-034](bugs/BUG-034-FIXED.md) | FIXED 2026-05-22 | layout | transform-origin 50% 50% default not resolved against box size — pivot at (0,0) instead of center |
| [BUG-035](bugs/BUG-035-FIXED.md) | FIXED 2026-05-22 | layout | ::before/::after pseudo-elements не генерируются в box_tree (реализация частичная) |
| [BUG-036](bugs/BUG-036-FIXED.md) | FIXED 2026-05-26 | layout | border-radius: % значения (50%, etc.) не резолвятся → radius=0; только px работает |
| [BUG-037](bugs/BUG-037-FIXED.md) | FIXED 2026-05-26 | paint | CSS filter effects не применяются визуально (grayscale/sepia/blur/etc.) — shared filter_uniform перезаписывался; fix: per-pass буфер через mapped_at_creation |
| [BUG-038](bugs/BUG-038-FIXED.md) | FIXED 2026-05-26 | layout | list-style-position: inside — маркер занимал отдельную строку; li высотой 2× от нормы; fix: не продвигать child_y, сдвигать InlineRun вправо на marker_w |
| [BUG-039](bugs/BUG-039-FIXED.md) | FIXED 2026-05-26 | paint | dashed/dotted border mismatch vs Chrome/Edge: dash ratio 3:1→Skia algo, corner squares→circle quads for dotted, 1px linear SDF AA |
| [BUG-040](bugs/BUG-040-FIXED.md) | FIXED 2026-05-27 | layout | table layout unit tests assume direct `<tr>` children of `<table>`; html-full-tree-builder now injects implicit `<tbody>` breaking them |
| [BUG-041](bugs/BUG-041-FIXED.md) | FIXED 2026-05-27 | css-parser | style::tests::line_clamp_integer_value / _standard_property / _not_inherited fail: CSS rule `div { -webkit-line-clamp: 3 }` produces None — test accesses doc.root().children[0] which is `<html>` after full HTML5 parsing, so rule doesn't match `<div>` |
| [BUG-042](bugs/BUG-042-FIXED.md) | FIXED 2026-05-29 | js | QuickJsRuntime missing JsRuntime::resume() impl — all lumen-js tests fail to compile |
| [BUG-043](bugs/BUG-043-FIXED.md) | FIXED 2026-05-29 | paint | lumen-paint test suite красный (19 падений): устаревшие golden + overflow coercion + half-leading |
| [BUG-044](bugs/BUG-044-FIXED.md) | FIXED 2026-05-29 | shell | lumen-shell не компилируется: non-exhaustive match по DisplayCommand — новые варианты PushMaskLayer/PopMaskLayer/DrawSvgPath/BoxModelOverlay |
| [BUG-045](bugs/BUG-045-FIXED.md) | FIXED 2026-05-29 | layout | backdrop-filter не создавал stacking context |
| [BUG-046](bugs/BUG-046-FIXED.md) | FIXED 2026-05-30 | layout | 3 устаревших теста lumen-layout --lib: webp теперь декодируется → picture-тесты обновлены |
| [BUG-047](bugs/BUG-047-FIXED.md) | FIXED 2026-05-30 | layout | НЕ баг (мисдиагноз): line-clamp реально усекает контент — тест переписан на ground-truth |
| [BUG-048](bugs/BUG-048-FIXED.md) | FIXED 2026-05-30 | shell | lumen-shell не компилируется: non-exhaustive match — новый вариант DrawScrollbar |
| [BUG-049](bugs/BUG-049-FIXED.md) | FIXED 2026-05-30 | shell | lumen-shell не компилируется: non-exhaustive match — новый вариант PageBreak |
| [BUG-050](bugs/BUG-050-FIXED.md) | FIXED 2026-05-31 | network | doctest mock.rs:16 не компилировался — `use NetworkTransport` не импортирован |
| [BUG-051](bugs/BUG-051-FIXED.md) | FIXED 2026-05-31 | layout | abs-pos с top+bottom+height:auto (inset:0) схлопывался в height 0 |
| [BUG-052](bugs/BUG-052-FIXED.md) | FIXED 2026-05-31 | paint/cpu_raster | DrawBorder anti_alias:true → паника в debug-профиле для sub-pixel рамок |
| [BUG-053](bugs/BUG-053-FIXED.md) | FIXED 2026-06-02 | shell | `cargo build -p lumen-shell --features quickjs` не компилировался: trait PersistentJs потерял декларации |
| [BUG-054](bugs/BUG-054-FIXED.md) | FIXED 2026-06-04 | network | stale_pooled_connection_triggers_retry падает на Windows (WSAECONNRESET) |
| [BUG-055](bugs/BUG-055-FIXED.md) | FIXED 2026-06-04 | layout | tests::collect_picture_unsupported_type_falls_back: AVIF теперь поддерживается |
| [BUG-056](bugs/BUG-056-FIXED.md) | FIXED 2026-06-03 | shell | font_registry used after move in parse_and_layout: clippy E0382 |
| [BUG-057](bugs/BUG-057-FIXED.md) | FIXED 2026-06-03 | paint | wgpu Vulkan crash on first render — fix: DX12 backend по умолчанию на Windows |
| [BUG-058](bugs/BUG-058-FIXED.md) | FIXED 2026-06-04 | layout | display:contents не сглажен перед lay_out: паника при открытии cnn.com |
| [BUG-059](bugs/BUG-059-FIXED.md) | FIXED 2026-06-04 | font | WOFF2-декодер отклоняет шрифты с контурами из 0 точек |
| [BUG-060](bugs/BUG-060-FIXED.md) | FIXED 2026-06-04 | font | WOFF2-декодер обрывается с «unexpected end of font data» — routing потоков |
| [BUG-061](bugs/BUG-061-FIXED.md) | FIXED 2026-06-04 | driver | test_32_list_markers падал: ожидания не обновлены после добавления секций |
| [BUG-062](bugs/BUG-062-FIXED.md) | FIXED 2026-06-04 | network | clippy «very complex type» в doh.rs → type alias DnsCacheMap |
| [BUG-063](bugs/BUG-063-FIXED.md) | FIXED 2026-06-04 | layout | clippy: manual_clamp, dead_code, collapsible_if в mathml.rs |
| [BUG-064](bugs/BUG-064-FIXED.md) | FIXED 2026-06-08 | driver | test_33_multi_column падал: ожидания не обновлены после изменения высот |
| [BUG-065](bugs/BUG-065-FIXED.md) | FIXED 2026-06-04 | shell | Клик по `<a href>` не срабатывал: hit-test не вычитал TAB_BAR_HEIGHT |
| [BUG-066](bugs/BUG-066-FIXED.md) | FIXED 2026-06-07 | paint | render_tile() без cfg(cpu-render) вызывает cpu_raster → clippy падает |
| [BUG-067](bugs/BUG-067-FIXED.md) | FIXED 2026-06-08 | js | EventTarget не определён глобально → `class X extends EventTarget` бросает ReferenceError |
| [BUG-068](bugs/BUG-068-FIXED.md) | FIXED 2026-06-08 | shell | clippy: collapsible_if в reader_view.rs |
| [BUG-069](bugs/BUG-069-FIXED.md) | FIXED 2026-06-08 | image | collect_picture_unsupported_type_falls_back: stub-форматы jxl/heic/heif убраны из supported_mime_types |
| [BUG-070](bugs/BUG-070-FIXED.md) | FIXED 2026-06-08 | js | Дубликат BUG-067 (тот же корень: EventTarget) |
| [BUG-071](bugs/BUG-071-FIXED.md) | FIXED 2026-06-08 | mcp | MockSession не реализует set_clock/set_rng_seed/freeze_fingerprint |
| [BUG-072](bugs/BUG-072-FIXED.md) | FIXED 2026-06-08 | js | Form Constraint Validation API: ReferenceError «HTMLInputElement is not defined» |
| [BUG-073](bugs/BUG-073-FIXED.md) | FIXED 2026-06-08 | js | chrome_runtime_absent ломается: window.chrome.runtime ставился безусловно |
| [BUG-074](bugs/BUG-074-FIXED.md) | FIXED 2026-06-08 | layout | height:100% на flex-item не резолвится → TEST-67 bar рендерится h=0 |
| [BUG-075](bugs/BUG-075-FIXED.md) | FIXED 2026-06-08 | layout | display:table без явной ширины растягивается до контейнера вместо shrink-to-fit |
| [BUG-076](bugs/BUG-076-FIXED.md) | FIXED 2026-06-11 | paint | box-shadow blur spread ~1% deviation — TEST-15: 1.06% |
| [BUG-077](bugs/BUG-077-FIXED.md) | FIXED 2026-06-09 | image/paint | femtovg-бэкенд алиасинг при downscale — area avg ресемплинг |
| [BUG-078](bugs/BUG-078-FIXED.md) | FIXED 2026-06-11 | layout/paint | object-fit contain/cover image quality ~13% deviation — TEST-19: 12.68% |
| [BUG-079](bugs/BUG-079-FIXED.md) | FIXED 2026-06-14 | layout | quirks-bgcolor: hashless-hex quirk применялся к шортхенду `background:` |
| [BUG-080](bugs/BUG-080-FIXED.md) | FIXED 2026-06-11 | paint | border-style: residual dotted/dashed 3% deviation — TEST-21: 3.02% |
| [BUG-081](bugs/BUG-081-FIXED.md) | FIXED 2026-06-11 | layout | vertical-align: sub-pixel 0.99% deviation |
| [BUG-082](bugs/BUG-082-FIXED.md) | FIXED 2026-06-11 | paint | css-filter 33% deviation — TEST-30: 33.07% |
| [BUG-083](bugs/BUG-083-FIXED.md) | FIXED 2026-06-11 | layout/paint | list-markers residual 3.4% deviation |
| [BUG-084](bugs/BUG-084-FIXED.md) | FIXED 2026-06-12 | paint | border-radius residual 1.5% deviation — TEST-36: 1.50% |
| [BUG-085](bugs/BUG-085-OPEN.md) | OPEN | paint | linear/radial gradient 12% deviation — TEST-39: 12.05% |
| [BUG-086](bugs/BUG-086-FIXED.md) | FIXED 2026-06-09 | paint | conic-gradient: triangle-fan не обрезался по box + игнорировал repeating |
| [BUG-087](bugs/BUG-087-FIXED.md) | FIXED 2026-06-09 | paint | gradient layers ignored background-size/position/repeat — TEST-45: 17.29% |
| [BUG-088](bugs/BUG-088-FIXED.md) | FIXED 2026-06-12 | css-parser/layout | individual CSS transform properties (translate/rotate/scale) — TEST-46: 4.63% |
| [BUG-089](bugs/BUG-089-FIXED.md) | FIXED 2026-06-09 | paint | SVG basic shapes not rendered (rect/circle/ellipse/line) — TEST-47: 21.71% |
| [BUG-090](bugs/BUG-090-FIXED.md) | FIXED 2026-06-12 | layout | -webkit-line-clamp multi-line truncation — TEST-48: PASS 0.26% |
| [BUG-091](bugs/BUG-091-FIXED.md) | FIXED 2026-06-08 | paint | background-blend-mode: bottom layer wrapped in PushBlendMode — TEST-49: 30.62% |
| [BUG-092](bugs/BUG-092-FIXED.md) | FIXED 2026-06-12 | css-parser/layout | CSS variables var() in cascade — TEST-50: PASS 0.0001% |
| [BUG-093](bugs/BUG-093-FIXED.md) | FIXED 2026-06-11 | paint | scrollbar rendering TEST-51: threshold calibration замаскировала реальный дефект BUG-123 |
| [BUG-094](bugs/BUG-094-FIXED.md) | FIXED 2026-06-11 | paint | text-shadow with blur ~7% deviation — TEST-52: 6.82% |
| [BUG-095](bugs/BUG-095-FIXED.md) | FIXED 2026-06-09 | layout/paint | background-origin/background-clip positioning ~32% deviation — TEST-53: 31.78% |
| [BUG-096](bugs/BUG-096-FIXED.md) | FIXED 2026-06-09 | paint/layout | SVG `<path>` stroke tessellation not rendered — TEST-54: 9.50% |
| [BUG-097](bugs/BUG-097-FIXED.md) | FIXED 2026-06-09 | layout/paint | `<video>` placeholder: posterless video painted grey instead of transparent |
| [BUG-098](bugs/BUG-098-FIXED.md) | FIXED 2026-06-11 | paint | mix-blend-mode: ~14% deviation — PA-3: offscreen CPU mix_blend_rgba |
| [BUG-099](bugs/BUG-099-OPEN.md) | OPEN | js/paint | `<canvas>` 2D context not implemented — TEST-57: 28.66%; Phase 2 |
| [BUG-100](bugs/BUG-100-OPEN.md) | OPEN | layout | ::first-letter drop-cap / ::first-line not implemented — TEST-58: 6.04% |
| [BUG-101](bugs/BUG-101-OPEN.md) | OPEN | css-parser/paint | image-set() DPR selection / cross-fade() not implemented — TEST-59: 27.63% |
| [BUG-102](bugs/BUG-102-OPEN.md) | OPEN | paint | SVG stroke-linecap/linejoin/dasharray not rendered — TEST-60: 11.51% |
| [BUG-103](bugs/BUG-103-OPEN.md) | OPEN | js | View Transitions API not implemented — TEST-61: 99.53%; Phase 2 |
| [BUG-104](bugs/BUG-104-OPEN.md) | OPEN | layout | CSS Scroll Snap not implemented — TEST-62: 63.70%; Phase 1 |
| [BUG-105](bugs/BUG-105-OPEN.md) | OPEN | layout | CSS Masonry layout not implemented — TEST-63: 26.13%; Phase 2 |
| [BUG-106](bugs/BUG-106-FIXED.md) | FIXED 2026-06-09 | layout | TEST-64 table: missing UA heading defaults → h3 без размера и margin |
| [BUG-107](bugs/BUG-107-FIXED.md) | FIXED 2026-06-09 | layout | flex align-content: normal/stretch не распределял свободное пространство |
| [BUG-108](bugs/BUG-108-OPEN.md) | OPEN | paint | ::selection pseudo-element: background-color/color не применяются — TEST-66: 6.18% |
| [BUG-109](bugs/BUG-109-OPEN.md) | OPEN | css-parser/font | font-variation-settings: wght/wdth/slnt не передаются растеризатору — TEST-68: 3.21% |
| [BUG-110](bugs/BUG-110-FIXED.md) | FIXED 2026-06-14 | layout/paint | object-fit: SVG viewBox scaling ~8% deviation — TEST-70: 8.03% |
| [BUG-111](bugs/BUG-111-FIXED.md) | FIXED 2026-06-08 | paint/shell | lumen-paint/shell не компилировались после мержа A-2 CSS Custom Highlight API |
| [BUG-112](bugs/BUG-112-FIXED.md) | FIXED 2026-06-08 | driver | test_32_list_markers регрессия: P4 добавил 2 @counter-style списка |
| [BUG-113](bugs/BUG-113-FIXED.md) | FIXED 2026-06-09 | layout | TEST-53 row-2 drift ~24px: trailing cross_gap утекал в single-line flex |
| [BUG-114](bugs/BUG-114-OPEN.md) | OPEN | css-parser | `font` shorthand drops font-size/line-height — TEST-53 residual ~4px |
| [BUG-115](bugs/BUG-115-OPEN.md) | OPEN | css-parser | percent `background-size` not supported — TEST-45 residual |
| [BUG-116](bugs/BUG-116-FIXED.md) | FIXED 2026-06-09 | layout | auto table column widths: content-based sizing (CSS 2.1 §17.5.2) |
| [BUG-117](bugs/BUG-117-FIXED.md) | FIXED 2026-06-09 | layout | multi-column greedy assignment two bugs — TEST-33 16.14% |
| [BUG-118](bugs/BUG-118-FIXED.md) | FIXED 2026-06-09 | test/snapshot | snapshot_cpu reference PNGs outdated for 12 pages |
| [BUG-119](bugs/BUG-119-FIXED.md) | FIXED 2026-06-10 | test/html | raw U+0001 byte в `<head>` 17 тест-страниц → content shifted 20px |
| [BUG-120](bugs/BUG-120-FIXED.md) | FIXED 2026-06-10 | layout/text | C0 control chars render as 1-line text box instead of invisible |
| [BUG-121](bugs/BUG-121-FIXED.md) | FIXED 2026-06-10 | test/driver | snapshot_vs_edge gate red: wgpu vs femtovg бэкенды дают разный результат |
| [BUG-122](bugs/BUG-122-FIXED.md) | FIXED 2026-06-15 | test/paint | flaky compositor tests: wall-clock deadline зависел от планировщика ОС |
| [BUG-123](bugs/BUG-123-FIXED.md) | FIXED 2026-06-11 | paint | scroll container's own bg+border clipped by its own overflow scissor |
| [BUG-124](bugs/BUG-124-OPEN.md) | OPEN | layout/paint | TEST-51 residual 1.09%: fractional layout Y coords vs Edge pixel snapping |
| [BUG-125](bugs/BUG-125-OPEN.md) | OPEN | layout/paint | CSS Motion Path L1 (offset-path/offset-distance/offset-rotate) — TEST-76: 3.18% |
| [BUG-126](bugs/BUG-126-OPEN.md) | OPEN | layout | CSS Anchor Positioning L1 (anchor-name/position-anchor) — TEST-77: 53.45% |
| [BUG-127](bugs/BUG-127-OPEN.md) | OPEN | layout/js | CSS Scroll-Driven Animations L1 (scroll-timeline/view-timeline) — TEST-78: 12.02% |
| [BUG-128](bugs/BUG-128-OPEN.md) | OPEN | font | text-underline TEST-79: 6.78% — font-parity issue (serif vs sans), кандидат в KNOWN_DEBTORS |
| [BUG-129](bugs/BUG-129-FIXED.md) | FIXED 2026-06-14 | layout | CSS Tables border-collapse: collapse — TEST-80 16.81% |
| [BUG-130](bugs/BUG-130-FIXED.md) | FIXED 2026-06-13 | paint | view-transition-name: TEST-81 32.47% — ложная причина, реальная = BUG-141 |
| [BUG-131](bugs/BUG-131-FIXED.md) | FIXED 2026-06-13 | paint | INTERACTION TEST-100 (transform×overflow) 9.57%: overflow-клип закрывался до дочернего SC |
| [BUG-132](bugs/BUG-132-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-101 (border-radius×overflow) 4.04%: PushClipRoundedRect добавлена |
| [BUG-133](bugs/BUG-133-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-102 (opacity×z-index) 17.04%: per-draw alpha вместо offscreen |
| [BUG-134](bugs/BUG-134-FIXED.md) | FIXED 2026-06-15 | paint | INTERACTION TEST-103 (filter×transform): ложная регрессия — устаревший бинарь |
| [BUG-135](bugs/BUG-135-OPEN.md) | OPEN | paint | INTERACTION TEST-104 (mask×gradient×radius) 51.97% |
| [BUG-136](bugs/BUG-136-FIXED.md) | FIXED 2026-06-13 | layout | INTERACTION TEST-105 (float/clear×margin) 4.84%: три дефекта float-раскладки |
| [BUG-137](bugs/BUG-137-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-106 (transform×z-index) 4.02%→PASS 0.02% |
| [BUG-138](bugs/BUG-138-FIXED.md) | FIXED 2026-06-13 | paint | INTERACTION TEST-107 (shadow×radius×overflow): box-shadow на скруглённом боксе — квадратный FillRect |
| [BUG-139](bugs/BUG-139-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-108 (вложенные transform) 4.62%: PopTransform эмитировался до дочерних SC |
| [BUG-140](bugs/BUG-140-FIXED.md) | FIXED 2026-06-13 | paint | INTERACTION TEST-109 (clip-path×transform×radius) 14.10%→4.80% |
| [BUG-141](bugs/BUG-141-FIXED.md) | FIXED 2026-06-13 | layout | TEST-71 17.83%: flex align-items:center в non-wrap контейнере игнорировал cross size |
| [BUG-142](bugs/BUG-142-OPEN.md) | OPEN | paint/shadow-dom | :host / ::slotted rendering diverges — TEST-72: 11.24% |
| [BUG-143](bugs/BUG-143-OPEN.md) | OPEN | layout | masonry-auto-flow placement diverges — TEST-75: 16.97% |
| [BUG-144](bugs/BUG-144-OPEN.md) | OPEN | paint | CSS filter visual rendering rows 1-3 — TEST-30: 18.81% residual |
| [BUG-145](bugs/BUG-145-FIXED.md) | FIXED 2026-06-12 | paint | РЕГРЕССИЯ: offscreen filter layer сайзился по bounds → viewport stretch |
| [BUG-146](bugs/BUG-146-FIXED.md) | FIXED 2026-06-12 | paint | TEST-15 box-shadow регрессия 1.06%→6.58%: blur-FBO без FLIP_Y |
| [BUG-147](bugs/BUG-147-FIXED.md) | FIXED 2026-06-12 | shell | clippy -D warnings fails: redundant use, dead code, unnecessary cast |
| [BUG-148](bugs/BUG-148-FIXED.md) | FIXED 2026-06-12 | shell | test hit_page_range_field fails: W-2b добавил строку Scale, row сдвинулся |
| [BUG-149](bugs/BUG-149-FIXED.md) | FIXED 2026-06-13 | test/snapshot | snapshot_cpu красный: эталоны устарели после PA-5 dashed/dotted бордеров |
| [BUG-151](bugs/BUG-151-FIXED.md) | FIXED 2026-06-13 | layout | Parent-first-child margin collapse не применяется (CSS 2.1 §8.3.1) |
| [BUG-152](bugs/BUG-152-FIXED.md) | FIXED 2026-06-13 | layout | anon_style клонирует float_side/clear/position родителя → анонимный бокс флоатится |
| [BUG-153](bugs/BUG-153-FIXED.md) | FIXED 2026-06-14 | test | CPU-эталоны протухли: регрессия от BUG-151 + 1024-byte сдвиг |
| [BUG-154](bugs/BUG-154-FIXED.md) | FIXED 2026-06-15 | layout | mix_polar путает индекс hue для LCH/Oklch (hue на индексе 2, не 0) |
| [BUG-155](bugs/BUG-155-FIXED.md) | FIXED 2026-06-15 | js | perf_observer_lcp_entry: index out of bounds — element_nid=42 за пределами тест-дока |
| [BUG-156](bugs/BUG-156-FIXED.md) | FIXED 2026-06-15 | paint/layout | ЛОЖНАЯ РЕГРЕССИЯ TEST-27: устаревший lumen.exe в прогоне 06-15 |
| [BUG-157](bugs/BUG-157-FIXED.md) | FIXED 2026-06-15 | paint | ЛОЖНАЯ РЕГРЕССИЯ TEST-40: та же причина — устаревший lumen.exe |
| [BUG-158](bugs/BUG-158-FIXED.md) | FIXED 2026-06-15 | layout | карточки новостей lenta.ru налезают друг на друга: `flex:1` (flex-basis:0) item в column-flex схлопывался в height=0 — нет automatic minimum size (§4.5) |
| [BUG-159](bugs/BUG-159-FIXED.md) | FIXED 2026-06-15 | paint | z-indexed потомок плоского overflow:auto scroll-контейнера сбегал из scroll-слоя (рисовался после PopScrollLayer) → fill_buckets переустанавливает scroll-слой для дочерних SC (кроме fixed/sticky) |
| [BUG-160](bugs/BUG-160-FIXED.md) | FIXED 2026-06-15 | font | WOFF2-шрифты не декодируются («unexpected end of font data»), спасает только woff-fallback — затрагивает большинство сайтов |
| [BUG-161](bugs/BUG-161-FIXED.md) | FIXED 2026-06-15 | network | HTTP/2 HPACK «dynamic table size update exceeds negotiated max» → ya.ru не грузится |
| [BUG-162](bugs/BUG-162-FIXED.md) | FIXED 2026-06-15 | encoding | детектор кодировки выдаёт ibm866 на чистом ASCII (example.com) вместо UTF-8 |
| [BUG-163](bugs/BUG-163-OPEN.md) | OPEN | shell/layout | `<link rel=preload as=image>` хинты не дозагружаются и не рендерятся: на lenta.ru 94 preload-картинки игнорируются (в DOM нет `<img>` — контент строит JS) |
| [BUG-164](bugs/BUG-164-OPEN.md) | OPEN | shell/js | внешние `<script src>` не скачиваются и не исполняются (collect_inline_scripts берёт только инлайны) → JS бандлы (lenta.ru owlBundle.js и т.д.) не работают, первопричина BUG-163 |

---

## Ограничения Phase 0 (не баги — запланировано позже)

Тесты, перешедшие в PASS: TEST-27 (direction-rtl), TEST-29 (@container), TEST-35 (grid), TEST-37 (float).

| Фича | Фаза | TEST (последний прогон) |
|---|---|---|
| `position:absolute/fixed/relative` | Phase 1 | — |
| CSS-анимации / transitions | Phase 2 | — |
| HiDPI / DPR-масштабирование | Phase 1 | — |
| `column-count` / `column-width` (multi-column) | Phase 1 | TEST-33: 16.14% → BUG-083 |
| `mask-image` | Phase 1 | TEST-26: 20.26% → BUG (Phase 1) |
| `contain:` CSS containment | Phase 1 | TEST-28: 1.82% → BUG (Phase 1) |
| Form controls UA styles | Phase 1 | TEST-34: 4.78% → BUG (Phase 1) |
| `clip-path: circle/ellipse/polygon` — точная форма | Phase 1 | TEST-31: 8.85% (bbox работает) |
| SVG rendering | Phase 1 | TEST-47: FIXED 2026-06-09 → BUG-089 |
| SVG `<path>` stroke | Phase 1 | TEST-54: FIXED 2026-06-09 → BUG-096 |
| SVG stroke advanced | Phase 1 | TEST-60: 11.51% → BUG-102 |
| `<canvas>` 2D context | Phase 2 | TEST-57: 28.66% → BUG-099 |
| View Transitions API | Phase 2 | TEST-61: 99.53% → BUG-103 |
| CSS Scroll Snap | Phase 1 | TEST-62: 63.70% → BUG-104 |
| CSS Masonry | Phase 2 | TEST-63: 26.13% → BUG-105 |
