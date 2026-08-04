# BUG-128

**Статус:** OPEN (DEBTOR) — механизм резолва и дефолт документа починены,
остался измеритель конкретных системных семейств + нередуцируемая
глиф-парити с Edge
**Компонент:** font
**Файл:** `crates/engine/paint/src/lib.rs` (`MultiFontMeasurer` не знает
конкретных системных семейств)

## Описание

text-underline TEST-79: 6.78%. РАССЛЕДОВАНО 2026-06-14 (P3): подчёркивание НЕ geometry-баг — вертикаль в пределах 1–2px от Edge, ~3px gap text→underline в обоих. Расхождение целиком из-за дефолтного шрифта: Edge рендерит serif (Times), Lumen — Inter (sans). Из 5.80% CPU-диффа 4.35% — глифы/ширина текста (нередуцируемо), лишь 1.46% в полосах underline и тоже font-width-driven.

## Что починено 2026-08-04 (P3, ветка `p3-bug-128-generic-fonts`)

Предпосылка «Lumen не умеет рисовать не-Inter» больше не верна: **CSS
generic-семейства резолвятся в системные шрифты**.

Было: `serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/`system-ui` явно
пропускались в трёх местах резолва (`Renderer::resolve_face_id_uncached`,
`Renderer::prefetch_faces_parallel`, `femtovg_backend::resolve_font_chain` +
varied-path) с комментарием «Phase 0 без per-generic-fallback таблицы», а
`MultiFontMeasurer` знал только @font-face-семьи. Любой `font-family: serif`
и рисовался, и мерился bundled Inter-ом.

Стало:
- `lumen_core::ext::generic_family_candidates` — платформенная таблица
  кандидатов (Windows: serif → Times New Roman, sans-serif → Arial,
  monospace → Consolas, cursive → Comic Sans MS, fantasy → Impact,
  system-ui → Segoe UI; свои списки для macOS и Linux) +
  `FontProvider::pick_generic_face` поверх неё;
- оба текстовых пути рендера ходят через общий `pick_family_face`, поэтому
  wgpu- и femtovg-бэкенды выбирают один и тот же face;
- `lumen_paint::GenericFaceSet` — метрики выбранных face-ов, строятся один
  раз на процесс и шарятся `Arc`-ом между пересборками `MultiFontMeasurer`
  (измеритель пересоздаётся на каждый relayout);
- shell: единая точка сборки измерителя `page_measurer()` для всех
  layout-путей (полный / инкрементальный / restyle) — раньше страницы без
  web-шрифтов вообще шли мимо `MultiFontMeasurer`;
- UA stylesheet: `font-family: monospace` для `<pre>`/`<code>`/`<kbd>`/
  `<samp>`/`<tt>`/`<listing>`/`<xmp>`/`<plaintext>` (HTML §15.3.2);
- intrinsic-ширины (min/max-content, list-marker, ширина пробела в
  wrap/pretty_wrap) считаются тем же семейством, что и перенос строк —
  иначе shrink-to-fit бокс мерился Inter-ом, а текст в нём набирался
  Times/Consolas;
- `lumen_font::shared_system_index()` — скан директорий шрифтов один раз на
  процесс, а не на каждый `FontRegistry` (страницу).

Полный графический прогон (152 теста, 2026-08-04): FAIL-набор совпадает с
main, регрессий нет; улучшения на текстовых тестах — TEST-97 −1.58,
TEST-95 −0.74, TEST-52 −0.62, TEST-46 −0.53, TEST-32 −0.34, TEST-58 −0.30,
TEST-145 −0.29, TEST-53 −0.23 п.п. и далее по мелочи.

## Что починено 2026-08-04, ч.2 (P3, ветка `p3-bug-128-default-serif`)

Дефолт документа. Было: корневой `ComputedStyle::root()` нёс **пустой**
`font_family`. Пустой список в ОБОИХ рендер-бэкендах зарезервирован за
chrome UI (DS-4: `Renderer::resolve_face_id` и
`FemtovgBackend::resolve_font_chain` при пустом списке возвращают bundled
Golos Text) — то есть страница без объявленного `font-family` рисовалась
не «bundled Inter», как считалось раньше, а **шрифтом браузерного
интерфейса**. UA-таблица Edge/Chrome/Firefox даёт корню `serif`
(на Windows — Times New Roman).

Стало:
- `lumen_layout::style::DEFAULT_FONT_FAMILY` = `"serif"` + хелпер
  `default_font_family()`; используется в `ComputedStyle::root()` и в
  quirks-сбросе шрифта таблицы (`apply_ua_table_font_reset`), который
  раньше сбрасывал в тот же пустой список. Конкретный face выбирает
  механизм ч.1 (`pick_generic_face` / `GenericFaceSet`);
- инвариант «у контента `font_family` НИКОГДА не пуст, пустой бывает
  только у chrome-овых `DrawText`» стал правдой — комментарии в
  `renderer.rs` и `femtovg_backend.rs` ссылались на него как на факт ещё
  до этого фикса;
- chrome-документ (`crates/chrome`, рендерится тем же движком, поэтому
  тоже стартует с `ComputedStyle::root()`) на serif НЕ уехал и правки не
  потребовал: ассет сам объявляет `body{font-family:var(--font-ui)}`, а
  всё содержимое chrome наследует от `<body>`. Первая редакция фикса
  добавляла в `lumen_chrome::UA_DEFAULTS` пин `html{font-family:'Golos
  Text'}` — он оказался мёртвым (правило на `html` проигрывает
  авторскому на `body`) и был снят; вместо него в крейте лежит тест
  `ua_defaults_need_no_font_family_because_the_asset_sets_its_own`,
  который упадёт, если ассет когда-нибудь перегенерируют без этого
  объявления.

Полный графический прогон (152 теста, 2026-08-04): 24 снят с учёта
(0.5023% → 0.47%, цель достигнута), ратчет 71 4.53→1.76 и 83 11.91→9.82;
улучшения без ратчета — 79 6.76→5.64, 64 8.99→7.66, 52 4.25→2.50,
97 2.78→1.34, 122 11.19→9.69, 137 4.74→2.95, 136 1.98→0.50, 117 2.23→1.13,
46 1.96→1.32 и далее по мелочи. Регрессий нет. TEST-61 (11.2%) — красный
ратчет, но **предсуществующий**: A/B показал 11.37% ещё до влития ч.1,
разбор перенесён в BUG-103.

## Что осталось (это и есть сам баг)

1. **`MultiFontMeasurer` не знает конкретных системных семейств.**
   `font-family: "Times New Roman"` / `Arial` рендер выбирает правильно, а
   измеритель меряет bundled Inter-ом → ширины строк не совпадают с
   нарисованными глифами. Косвенно работает (`Arial, sans-serif` меряется
   Arial-ом через generic-хвост), но `font-family: Arial` без
   generic-хвоста — нет. Лечится тем же приёмом, что `GenericFaceSet`:
   ленивый кэш метрик по имени семейства поверх `FontProvider::pick_face`.
2. **Нередуцируемая глиф-парити с Edge.** Оставшиеся проценты должников
   класса font-parity (TEST-79 5.64% и др.) — это разница растеризации/
   хинтинга одного и того же Times New Roman, а не выбор шрифта. Закроется
   только вместе с FP-1 (единая политика рендера глифов), не точечным
   фиксом.

Записи `KNOWN_DEBTORS` с тегом BUG-128 (32/34/46/52/64/66/67/79/80/82/84/
95/97) держатся на пункте 2 — поэтому баг остаётся OPEN.
