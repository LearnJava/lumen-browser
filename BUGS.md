# BUGS.md — Баг-трекер Lumen Browser

Живой список известных багов движка. История прогонов — в `graphic_tests/results/*.json` (коммитируются).

**Как добавить баг:**
1. Создай файл `bugs/BUG-NNN-OPEN.md` (следующий номер по счёту, сейчас BUG-225)
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
| [BUG-085](bugs/BUG-085-OPEN.md) | OPEN | paint | linear/radial gradient — TEST-39: 12.05%→1.62% (femtovg_stops: repeating-градиенты повторяются + hard-stop хвост дозаполняется до 1.0). Остаток DEBTOR: 256-тексельная квантизация градиент-текстуры femtovg на repeating-границах + radial AA vs Edge |
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
| [BUG-102](bugs/BUG-102-FIXED.md) | FIXED 2026-06-17 | paint | SVG stroke-width/dasharray молча терялись в standards-mode (unitless user units) + join-шипы; TEST-60 11.51%→1.41%, TEST-54 5.58%→2.30% |
| [BUG-103](bugs/BUG-103-OPEN.md) | OPEN | js | View Transitions API not implemented — TEST-61: 99.53%; Phase 2 |
| [BUG-104](bugs/BUG-104-FIXED.md) | FIXED 2026-06-19 | layout | TEST-62 63.70%→2.32%: реальная причина — column flex-grow не распределял free space. `lay_out_flex` хардкодил `container_main=0`/`free_space=0` для column → `.right-col` дети `flex:1` схлопывались в h≈0. Фикс: `explicit_main` для column (явная height или растяжение родителем re-layout). Геометрия пиксель-точна (diff: все заливки идентичны). Остаток 2.32% = font-parity (BUG-128) метки секций + border-radius edge-AA (BUG-176) → KNOWN_DEBTORS. box_tree.rs:5097/7191 |
| [BUG-105](bugs/BUG-105-OPEN.md) | OPEN | layout | CSS Masonry layout not implemented — TEST-63: 26.13%; Phase 2 |
| [BUG-106](bugs/BUG-106-FIXED.md) | FIXED 2026-06-09 | layout | TEST-64 table: missing UA heading defaults → h3 без размера и margin |
| [BUG-107](bugs/BUG-107-FIXED.md) | FIXED 2026-06-09 | layout | flex align-content: normal/stretch не распределял свободное пространство |
| [BUG-108](bugs/BUG-108-FIXED.md) | FIXED 2026-06-17 | layout | TEST-66 5.24%→1.08%: реальная причина — parent↔last-child bottom margin не коллапсил (CSS 2.1 §8.3.1), свотчи уезжали вниз +30px/секция. Остаток 1.08% — текст (font-parity, rule 3) + border-radius AA |
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
| [BUG-126](bugs/BUG-126-OPEN.md) | OPEN (DEBTOR) | layout | CSS Anchor Positioning L1 — placement фикснут (53.45% → 12.94%): position-area definite-size элементы прижимаются к якорю вместо растягивания на band (anchor.rs place_axis/align_in_band). 3×3 сетка совпадает с Edge(position-area) пиксель-в-пиксель. Остаток-должник: тест использует устаревшее `inset-area` (Edge игнорирует, поддерживает только `position-area`) + span-ряд (Lumen спек-корректнее Edge). KNOWN_DEBTORS 12.94% |
| [BUG-127](bugs/BUG-127-OPEN.md) | OPEN | layout/js | CSS Scroll-Driven Animations L1 (scroll-timeline/view-timeline) — TEST-78: 12.02% |
| [BUG-128](bugs/BUG-128-OPEN.md) | OPEN | font | text-underline TEST-79: 6.78% — font-parity issue (serif vs sans), кандидат в KNOWN_DEBTORS |
| [BUG-129](bugs/BUG-129-FIXED.md) | FIXED 2026-06-14 | layout | CSS Tables border-collapse: collapse — TEST-80 16.81% |
| [BUG-130](bugs/BUG-130-FIXED.md) | FIXED 2026-06-13 | paint | view-transition-name: TEST-81 32.47% — ложная причина, реальная = BUG-141 |
| [BUG-131](bugs/BUG-131-FIXED.md) | FIXED 2026-06-13 | paint | INTERACTION TEST-100 (transform×overflow) 9.57%: overflow-клип закрывался до дочернего SC |
| [BUG-132](bugs/BUG-132-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-101 (border-radius×overflow) 4.04%: PushClipRoundedRect добавлена |
| [BUG-133](bugs/BUG-133-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-102 (opacity×z-index) 17.04%: per-draw alpha вместо offscreen |
| [BUG-134](bugs/BUG-134-FIXED.md) | FIXED 2026-06-15 | paint | INTERACTION TEST-103 (filter×transform): ложная регрессия — устаревший бинарь |
| [BUG-135](bugs/BUG-135-FIXED.md) | FIXED 2026-06-17 | paint | INTERACTION TEST-104 (mask×gradient×radius) 51.97% → 0.44% PASS (фикс BUG-183) |
| [BUG-136](bugs/BUG-136-FIXED.md) | FIXED 2026-06-13 | layout | INTERACTION TEST-105 (float/clear×margin) 4.84%: три дефекта float-раскладки |
| [BUG-137](bugs/BUG-137-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-106 (transform×z-index) 4.02%→PASS 0.02% |
| [BUG-138](bugs/BUG-138-FIXED.md) | FIXED 2026-06-13 | paint | INTERACTION TEST-107 (shadow×radius×overflow): box-shadow на скруглённом боксе — квадратный FillRect |
| [BUG-139](bugs/BUG-139-FIXED.md) | FIXED 2026-06-12 | paint | INTERACTION TEST-108 (вложенные transform) 4.62%: PopTransform эмитировался до дочерних SC |
| [BUG-140](bugs/BUG-140-FIXED.md) | FIXED 2026-06-13 | paint | INTERACTION TEST-109 (clip-path×transform×radius) 14.10%→4.80% |
| [BUG-141](bugs/BUG-141-FIXED.md) | FIXED 2026-06-13 | layout | TEST-71 17.83%: flex align-items:center в non-wrap контейнере игнорировал cross size |
| [BUG-142](bugs/BUG-142-FIXED.md) | FIXED 2026-06-17 | paint/shadow-dom | :host / ::slotted rendering diverges — TEST-72: 11.24% → 0.00% |
| [BUG-143](bugs/BUG-143-OPEN.md) | OPEN | layout | masonry-auto-flow placement diverges — TEST-75: 16.97% |
| [BUG-144](bugs/BUG-144-OPEN.md) | OPEN | paint | CSS filter/backdrop-filter — TEST-30: row-flip 16.42%→10.48% (FLIP_Y) + gradient hard-stop row 2 (BUG-085) 10.48%→7.56% + backdrop colour-matrix/combo больше не тёмные (CPU backdrop-пайплайн `apply_backdrop_filters`: blur через `box_blur_rgba`, без мёртвого filter_image-readback) 7.56%→4.36%. Остаток DEBTOR: box-blur≈Gaussian + edge-bleed на двух blur-картах (row 4 cards 1,5) + filter AA |
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
| [BUG-163](bugs/BUG-163-FIXED.md) | FIXED 2026-06-15 | shell/paint | lazy `<img>` на lenta.ru не рисовались: `LazyImageSlot` всегда красил серый placeholder даже после загрузки картинки + above-the-fold lazy-картинки не дозагружались на initial paint (proximity-check был только в relayout) |
| [BUG-164](bugs/BUG-164-FIXED.md) | FIXED 2026-06-15 | shell/js | внешние `<script src>` не скачиваются и не исполняются (collect_inline_scripts берёт только инлайны) → JS бандлы (lenta.ru owlBundle.js и т.д.) не работают, первопричина BUG-163 |
| [BUG-165](bugs/BUG-165-FIXED.md) | FIXED 2026-06-15 | layout | flex `align-content` сдвигал строку, не двигая поддерево item-ов: вложенный контент оставался на месте → items вылезали из контейнеров (TEST-65: 16.40%) |
| BUG-166 | FIXED 2026-06-16 | lumen-js | `video_bindings::tests::native_video_load_registers_pending` flaky on parallel run — two tests raced on the process-global `video_gif_store` singleton; serialized them with a `STORE_GUARD` mutex | crates/js/src/video_bindings.rs |
| [BUG-167](bugs/BUG-167-FIXED.md) | FIXED 2026-06-19 | shell | вход/выход Fullscreen API не пересчитывал вьюпорт: окно растягивалось на весь десктоп, но страница оставалась в исходном вьюпорте (~1024×720). Корень: `set_fullscreen` применяет размер асинхронно, `inner_size()` сразу после вызова ещё старый. Фикс: `fullscreen_resize_pending` + `arm_fullscreen_resize`/`poll_fullscreen_resize` (из `about_to_wait`) ждут смены `inner_size()` и прогоняют тот же resize+relayout, что `WindowEvent::Resized`; чистое решение `decide_fullscreen_poll` под 6 юнит-тестов | crates/shell/src/main.rs |
| [BUG-169](bugs/BUG-169-FIXED.md) | FIXED 2026-06-16 | network+shell | Linux: pre-existing clippy/test-сбои в `#[cfg(linux/macos)]` platform-коде (не ловились на Windows-dev). network/ctap2: private `descriptor_is_fido` в тестах (E0603), unnecessary `unsafe` (1128), collapsible `if` (1192). shell: unused imports `screen_capture.rs:16`, dead `entry_from_path` `file_dialog.rs:116`. Починены как unblock гейта PH1-2a. | crates/network/src/ctap2.rs:1128 |
| [BUG-170](bugs/BUG-170-OPEN.md) | FIXED 2026-06-16 | shell+font | `@font-face` web-шрифты блокируют первый paint: `load_font_faces` качает все woff2 до layout (FOUT не реализован). Надо `font-display: swap` — рисовать фолбэком сразу, подменять web-шрифт в фоне с relayout. `font-display` уже парсится (css-parser:2199), не используется. | crates/shell/src/main.rs:3067 |
| [BUG-171](bugs/BUG-171-FIXED.md) | FIXED 2026-06-19 | shell | **Этап 1 ✅** (префетч-кэш `prefetch::PREFETCH_CACHE`): внешние скрипты + linked CSS прогреваются в фоне во время streaming, сеть снята с UI-потока. **Этап 2 ✅**: весь финальный pipeline вынесен с UI-потока — в `LoadEvent::LoadDone` `render_bytes` (fetch скриптов → QuickJS → fetch+декод картинок/CSS/шрифтов → layout) исполняется на `std::thread::spawn`, готовый результат прилетает назад как `LoadEvent::RenderDone(Box<RenderOutcome>, gen)` и применяется на UI через `apply_loaded_page`. Окно остаётся отзывчивым всю CPU-фазу. Разблокировано B-1/ADR-014 (`QuickJsRuntime` — `Send`-хэндл): JS-контекст создаётся на рендер-потоке и пересылается на UI. `PersistentJs: Send`, `hyp_provider: Arc<…>`, generation-guard отбрасывает устаревшую навигацию. | crates/shell/src/main.rs:7322 |
| [BUG-172](bugs/BUG-172-FIXED.md) | FIXED 2026-06-19 | shell | Картинки качались дважды на streaming-страницах: PH1-2c `spawn_stream_image_loads` грузил прогрессивно, финальный `fetch_and_decode_images` качал их же заново. Закрыто общим per-load кэшем `image_cache::IMAGE_CACHE` (`DecodedImageCache`): оба пути декодируют через `get_or_decode`/`get_or_decode_current` с единой fetch+decode-логикой (`decode_image`), generation-scoped + in-flight дедуп (как `prefetch::PREFETCH_CACHE`). Cache-hit отдаёт уже декодированные пиксели — финальный проход не трогает сеть/декодер; reset на старте навигации (`start_streaming_load`) и per-render (`render_source_to_png`). | crates/shell/src/image_cache.rs |
| [BUG-173](bugs/BUG-173-OPEN.md) | OPEN | paint | Остаток SVG `<path>` vs Edge после BUG-102: triangle-soup AA-швы (DrawSvgPath), stroke-edge AA, self-intersecting fill (ear_clip, незалитая bowtie), dash-on-curve. TEST-54 2.30% / TEST-60 1.41% — в KNOWN_DEBTORS | crates/engine/paint/src/svg_path.rs |
| [BUG-174](bugs/BUG-174-FIXED.md) | FIXED 2026-06-17 | layout | In-flow (inline-block) SVG `<path>` рисовался в raw user-координатах `d` без смещения на origin своего SVG-вьюпорта — все пути из разных SVG-ячеек схлопывались в верхний левый угол страницы (видны только те, что попали в свой clip). TEST-119 56.35% → 0.81%. | crates/engine/layout/src/box_tree.rs:1198 |
| [BUG-175](bugs/BUG-175-FIXED.md) | FIXED 2026-06-17 | paint | `border-radius` + `border`: рамка рисовалась 4 axis-aligned прямоугольниками без учёта радиуса → квадратные углы вокруг скруглённого фона (видно на пилюлях/кругах/эллипсах с бордером в TEST-36). Теперь однородная solid-рамка рисуется even-odd кольцом между внешним и внутренним скруглёнными rect. TEST-36 1.50% → 1.11%. | crates/engine/paint/src/backends/femtovg_backend.rs:1682 |
| [BUG-176](bugs/BUG-176-OPEN.md) | OPEN | paint | TEST-36 остаток 1.11% после BUG-175: edge-AA вдоль скруглённых границ (sub-pixel snapping vs Edge) + кубическая kappa-аппроксимация эллиптических углов (row 6, `border-radius: H/V`) отличается от точной дуги Edge. В KNOWN_DEBTORS. | crates/engine/paint/src/backends/femtovg_backend.rs:870 |
| [BUG-177](bugs/BUG-177-FIXED.md) | FIXED 2026-06-17 | layout | `height` на table-cell трактовался как фиксированный, а не минимальный (CSS 2.1 §17.5.3): ячейка с `height:64px` + content выше (52×32 блок + margin 16px → 64px content > 56px content-box) зажималась в 64px, content переполнял её в border-spacing-зазор, pitch строки был короче на величину переполнения и ошибка накапливалась вниз по таблице. Теперь used-height = max(specified, content). TEST-115 13.45% → 0.00%. | crates/engine/layout/src/box_tree.rs:5471 |
| [BUG-178](bugs/BUG-178-FIXED.md) | FIXED 2026-06-17 | layout | shrink-to-fit auto-width контейнера с несколькими `float`-детьми считал ширину как max ребёнка, а не сумму (CSS 2.1 §9.5.1 — флоаты стоят бок о бок). Float-обёртка с двумя `float:left` детьми по 200px сжималась до 200px → второй флоат переносился под первый вместо ряда. `preferred_inline_block_width` + `max_content_outer_width`: суммируем margin-box ширины float-детей, max берём только среди in-flow. TEST-51 9.91% → 1.09% (остаток = BUG-124, дробные Y-координаты). | crates/engine/layout/src/box_tree.rs:3750 |
| [BUG-179](bugs/BUG-179-FIXED.md) | FIXED 2026-06-17 | layout | flex-item с `flex-basis:auto` и без явной `width` использовал ширину из предварительного прохода (`item.rect.width` = ширина контейнера, т.к. блоки растягиваются). Элемент с `min-width:200px` в контейнере 600px получал base=600px → total_hyp=700px > 600px → ошибочный shrink → элемент 514px вместо 200px (второй столбец TEST-46 уезжал ~160px вправо). Фикс: `flex_auto_base_main_width` вычисляет max-content-width и ограничивает `min-width`/`max-width` (CSS Flexbox §9.2/§9.7). | crates/engine/layout/src/box_tree.rs:3932 |
| [BUG-180](bugs/BUG-180-FIXED.md) | FIXED 2026-06-17 | layout | TEST-18 21.21%→2.11%: блок-обёртка `<img>` не учитывала descent line-box baseline-выровненной inline-картинки («image bottom gap», CSS 2.1 §10.8) — каждый ряд картинок уезжал вверх на ~descent px, ошибка копилась вниз. `child_y += descent_px` после baseline replaced-ребёнка (box_tree.rs:5527). Остаток 2.11% = image-resampling AA (BUG-219) → KNOWN_DEBTORS |
| [BUG-181](bugs/BUG-181-FIXED.md) | FIXED 2026-06-20 | layout/paint | `object-fit` basic — расследовано: геометрия всех 5 режимов (fill/contain/cover/none/scale-down) + object-position верна (средние RGB ячеек совпадают с Edge ±0.1, лучший сдвиг 0,0, letterbox 19.5px корректен). TEST-19 9.05% = image-resampling AA на высокочастотном контенте (perceptron-диаграмма + agi rusty-текстура) → BUG-219, TEST-19 в KNOWN_DEBTORS. Регресс-тесты `bug181_*` (display_list.rs) фиксируют геометрию |
| [BUG-182](bugs/BUG-182-OPEN.md) | OPEN | layout/paint | `vertical-align` inline y-offset deviation — TEST-24: 0.98% |
| [BUG-183](bugs/BUG-183-FIXED.md) | FIXED 2026-06-17 | paint | `mask-image` gradient mask not implemented — TEST-26: 17.74% → 5.02% (остаток BUG-218 mask-mode:luminance) |
| [BUG-184](bugs/BUG-184-OPEN.md) | OPEN | paint | `clip-path` deviation — TEST-31: 0.59% |
| [BUG-185](bugs/BUG-185-OPEN.md) | OPEN | layout/paint | list `::marker` geometry deviation — TEST-32: 3.75% |
| [BUG-186](bugs/BUG-186-FIXED.md) | FIXED 2026-06-18 | layout | `multi-column` column fragmentation — TEST-33: 14.89% → 0.12% PASS |
| [BUG-187](bugs/BUG-187-OPEN.md) | OPEN (DEBTOR) | layout/paint | form controls static rendering — TEST-34: 4.78% → 3.02% → (этап 2) value-текст text-инпутов рисуется (email/password-маска/number/search + submit-лейбл, вертикальное центрирование, клиппинг по content-box), ipc 2.95%. Остаток DEBTOR: placeholder пустых полей + checkbox-галочка/radio-тик + font-parity лейблов |
| [BUG-188](bugs/BUG-188-OPEN.md) | OPEN | layout/paint | individual `translate`/`rotate`/`scale` transforms deviation — TEST-46: 4.63% |
| [BUG-189](bugs/BUG-189-OPEN.md) | OPEN | paint | SVG basic shapes deviation — TEST-47: 3.71% |
| [BUG-190](bugs/BUG-190-OPEN.md) | OPEN | paint | `background-blend-mode` deviation — TEST-49: 2.39% |
| [BUG-191](bugs/BUG-191-FIXED.md) | FIXED 2026-06-20 | paint | TEST-52 5.83%→4.25% DEBTOR: blur-пайплайн корректен — sigma=radius/2, GPU GaussianBlur на full-RT слое (halo не клипуется), multi-shadow и цветные glow совпадают с Edge по extent/intensity (glow-only и 20px кейсы проверены пиксельно). Остаток = font-parity (Edge serif vs Inter sans, rule 3) → KNOWN_DEBTORS BUG-128. Регресс-тест `text_shadow_blur_sigma_is_half_radius_for_test52_progression` |
| [BUG-192](bugs/BUG-192-OPEN.md) | OPEN | paint | `<video>` placeholder deviation — TEST-55: 0.89% |
| [BUG-193](bugs/BUG-193-FIXED.md) | FIXED 2026-06-17 | layout | TEST-64 13.89%→8.99%: `display:table`-обёртка не схлопывала margin с соседним блоком (CSS 2.1 §8.3.1) — bottom-margin таблицы + top-margin `<h3>` складывались (38.72px вместо 20px), нижняя таблица уезжала на ~19px. `is_block` теперь включает `Table` (box_tree.rs:5462). Остаток 8.99% = font-parity (BUG-128) → KNOWN_DEBTORS |
| [BUG-194](bugs/BUG-194-OPEN.md) | OPEN | layout | Flexbox `align-content` multi-line deviation — TEST-65: 1.33% |
| [BUG-195](bugs/BUG-195-OPEN.md) | OPEN | paint | `::selection` color override deviation — TEST-66: 1.07% |
| [BUG-196](bugs/BUG-196-FIXED.md) | FIXED 2026-06-18 | css-parser/layout | `::before`/`::after` с `content:attr()` не генерировались на flex/grid-контейнерах — TEST-67: 16.41% → 1.36% (KNOWN_DEBTORS, остаток font-parity) |
| [BUG-197](bugs/BUG-197-OPEN.md) | OPEN | layout | CSS Table `border-spacing` asymmetric deviation — TEST-69: 3.61% |
| [BUG-198](bugs/BUG-198-FIXED.md) | FIXED 2026-06-20 | layout/paint | inline `<svg>` ошибочно применял CSS `object-fit`/`object-position` к viewBox — Edge их игнорирует и фитит viewBox через `preserveAspectRatio` (SVG §7.8). Замена `compute_object_fit_transform` → `compute_preserve_aspect_ratio_transform` (box_tree.rs). Доп.: femtovg `draw_fill_rounded_rect` + `CornerRadii::clamped_to_box` схлопывали эллиптические углы `min(w/2,h/2)` → SVG `<ellipse>` рисовался «стадионом»; §5.5-клампинг сохраняет rx≠ry. TEST-70 7.82% → 1.63% → KNOWN_DEBTORS (BUG-176, kappa-AA эллиптических дуг) |
| [BUG-199](bugs/BUG-199-OPEN.md) | OPEN (DEBTOR) | layout | `@starting-style` — Lumen рендерит спек-корректное settled-состояние (обе коробки 200×200, opacity 1 после 0.4s entry-перехода); display-list геометрия/цвета идеальны. Edge headless --screenshot (без virtual-time) ловит entry-transition в полёте: transform у START-значения (box-a scale(0.5)→107px, box-b translateX(-80px)), opacity у END-значения (1) — взаимно несогласованный кадр = артефакт тайминга захвата Edge, не дефект движка. Совпасть требует проводки @starting-style в каскад (P4) + engine entry-переходов + воспроизведения невозможного кадра. KNOWN_DEBTORS 7.03%, тот же класс что BUG-126/TEST-77 |
| [BUG-200](bugs/BUG-200-FIXED.md) | FIXED 2026-06-19 | paint | TEST-80 collapse varied-width borders: thin cell's bg erased thick neighbour's shared border (ordered path emits cell bg+border interleaved in DOM order). Fix: redraw cell borders after all cell backgrounds in collapse mode (`display_list.rs` fill_buckets). Residual 9.91% = font-parity vertical drift (BUG-128) → KNOWN_DEBTORS |
| [BUG-201](bugs/BUG-201-OPEN.md) | OPEN | paint | SVG `<use>` cloning deviation — TEST-82: 5.00% |
| [BUG-202](bugs/BUG-202-FIXED.md) | FIXED 2026-06-17 | layout | TEST-83 14.02%→7.88%: реальная причина не scroll-behavior, а text-only inline-block без shrink-to-fit. `preferred_inline_block_width` мерил только дочерние боксы и игнорировал текст `InlineRun` (он в `segments`, не в `children`) → None → бокс растягивался на всю строку. Добавлена ветка измерения текста сегментов (box_tree.rs:3732). Pills `.pill` теперь обтягивают текст и текут в ряд. Остаток 7.88% = font-parity (BUG-128) → KNOWN_DEBTORS |
| [BUG-203](bugs/BUG-203-FIXED.md) | FIXED 2026-06-20 | paint | `text-decoration-skip-ink` gap geometry — TEST-84: 8.20% → 6.02% (DEBTOR BUG-128, font-parity) |
| [BUG-204](bugs/BUG-204-OPEN.md) | OPEN | layout | `anchor-name` basic stub deviation — TEST-85: 1.98% |
| [BUG-205](bugs/BUG-205-OPEN.md) | OPEN | layout | `position-anchor` fallback stub deviation — TEST-86: 2.12% |
| [BUG-206](bugs/BUG-206-OPEN.md) | OPEN | layout | `inset-area: none` stub deviation — TEST-87: 1.98% |
| [BUG-207](bugs/BUG-207-OPEN.md) | OPEN | layout | `anchor-name` nested stub deviation — TEST-88: 1.98% |
| [BUG-208](bugs/BUG-208-OPEN.md) | OPEN | layout | multiple `anchor-name` stub deviation — TEST-89: 1.98% |
| [BUG-209](bugs/BUG-209-OPEN.md) | OPEN | image | AVIF decoder not implemented — TEST-90: 2.75% |
| [BUG-210](bugs/BUG-210-FIXED.md) | FIXED 2026-06-18 | layout | CSS system color keywords resolved to wrong values — `system_color()` light-scheme значения приведены к Edge (Highlight #0078d7, LinkText/VisitedText/ActiveText #0066cc, ButtonBorder #000, GrayText #6d6d6d, AccentColor #0075ff, HighlightText white) + deprecated keywords (ThreeD*/Scrollbar) → standard per CSS Color 4 §6.3. TEST-92 15.59% → 0.90% (остаток = BUG-124, gdigrab суб-пиксель на границах). crates/engine/layout/src/style.rs:17903 |
| [BUG-211](bugs/BUG-211-OPEN.md) | OPEN | layout | `field-sizing: content` not implemented — TEST-93: 4.11% |
| [BUG-212](bugs/BUG-212-OPEN.md) | OPEN | font/layout | `font-size-adjust` not implemented — TEST-95: 3.39% |
| [BUG-213](bugs/BUG-213-OPEN.md) | OPEN | css-parser/layout | `counter-set` order deviation — TEST-97: 2.78% |
| [BUG-214](bugs/BUG-214-OPEN.md) | OPEN | paint | `accent-color` tint not implemented — TEST-110: 2.47% |
| [BUG-215](bugs/BUG-215-OPEN.md) | OPEN | layout | `shape-outside: path()` not implemented — TEST-113: 1.41% |
| [BUG-216](bugs/BUG-216-OPEN.md) | OPEN | css-parser/layout | CSS `quotes` + `open-quote`/`close-quote` deviation — TEST-117: 2.28% |
| [BUG-217](bugs/BUG-217-OPEN.md) | OPEN | css-parser | `prefers-contrast`/`prefers-reduced-data` media queries not matched — TEST-120: 3.26% |
| [BUG-218](bugs/BUG-218-FIXED.md) | FIXED 2026-06-19 | css-parser/paint | `mask-mode: luminance` not parsed/applied — TEST-26 luma-cell остаток 5.02% (P4); `emit_push_mask` bakes `luminance(rgb)·alpha` в stop alpha |
| [BUG-219](bugs/BUG-219-OPEN.md) | OPEN | image/paint | image downscale resampling pixel-parity vs Edge — TEST-18 остаток 2.11% (тонкий AA по всем фото после фикса BUG-180) → KNOWN_DEBTORS |
| [BUG-220](bugs/BUG-220-OPEN.md) | OPEN | paint | scroll-контейнер в ordered (stacking-context) пути теряет scrollbar: `box_layer_ops` эмитит `PushScrollLayer`/`PopScrollLayer`, но не `DrawScrollbar` (есть только в legacy `walk`) — display_list.rs:2481. Замечен при разборе BUG-202 |
| [BUG-221](bugs/BUG-221-FIXED.md) | FIXED 2026-06-20 | paint | CPU-бэкенд снимка (`render_to_image_cpu`, cpu_raster.rs) доведён до паритета с femtovg: border-radius (TEST-36 60.60%→0.47% — клампинг радиусов FillRoundedRect), радиальный градиент-круг вместо эллипса (TEST-39 10.68%→1.40%, TEST-26 5.02%→0.00%), реальная отрисовка `<img>` с object-fit + area-averaged downscale (TEST-18 52.22%→2.15%). Разблокирует полную замену gdigrab на `run.py --ipc` (TAB-7) |
| [BUG-222](bugs/BUG-222-FIXED.md) | FIXED 2026-06-19 | js/shell | WASM-реестр (`wasm::REGISTRY`) не очищается при разрушении JS-контекста: «утёкший» `Persistent` функции-импорта роняет QuickJS на `list_empty(&rt->gc_obj_list)` при teardown. Закрыт B-1/ADR-014: `js_thread_main` зовёт `wasm::clear_registry()` в teardown JS-потока до дропа `Runtime`. |
| [BUG-223](bugs/BUG-223-FIXED.md) | FIXED 2026-06-20 | network/ipc | `lumen-network-service` не компилируется (E0004): `match` в `network_service.rs:51` не покрывал таб-варианты `IpcRequest::{CreateTab,CloseTab,NavigateTab,Screenshot}` (TAB-4/5). Добавлены явные arm-ы → `IpcResponse::TabError` (сетевой процесс таб-ами не управляет), match исчерпывающий. `--workspace --all-targets` снова зелёный. |
| [BUG-224](bugs/BUG-224-FIXED.md) | FIXED 2026-06-20 | layout | НЕ регрессия движка: `test_33_multi_column` хранил устаревший ground-truth (660x88, atomic-раскладка до BUG-186). После BUG-186 (фрагментация колонок, TEST-33 14.89%→0.12%) Edge-корректная высота mc[4] = 660x64 (две 36px col-sm фрагментируются на 24px по 3 колонкам + span 16px). Подтверждено Edge `getBoundingClientRect → 64` и пиксельным паритетом TEST-33 ≈0.1%. p3-bug198 (diff чисто SVG) к multicol не прикасался. Юнит-тест ре-базлайнен 88→64. |

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
| SVG stroke advanced | Phase 1 | TEST-60: FIXED 2026-06-17 → BUG-102 (остаток → BUG-173, KNOWN_DEBTORS) |
| `<canvas>` 2D context | Phase 2 | TEST-57: 28.66% → BUG-099 |
| View Transitions API | Phase 2 | TEST-61: 99.53% → BUG-103 |
| CSS Scroll Snap | Phase 1 | TEST-62: FIXED 2026-06-19 → BUG-104 (column flex-grow; остаток 2.32% → BUG-128, KNOWN_DEBTORS) |
| CSS Masonry | Phase 2 | TEST-63: 26.13% → BUG-105 |
