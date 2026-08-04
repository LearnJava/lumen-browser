# BUG-128

**Статус:** OPEN (DEBTOR) — механизм резолва починен, остался дефолт документа
**Компонент:** font
**Файл:** `crates/engine/layout/src/style.rs` (дефолтный `font_family` пуст)

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

## Что осталось (это и есть сам баг)

Дефолтный `font-family` документа — пустой список, то есть bundled Inter
(sans). UA-таблица Edge/Chrome/Firefox даёт корневому элементу `serif`
(на Windows — Times New Roman). Поэтому страницы **без объявленного
`font-family`** (TEST-79 и большинство должников класса font-parity)
по-прежнему расходятся с эталоном: 7.13% на TEST-79 — одинаково на main и
на ветке с этим фиксом.

Почему не сделано здесь: смена дефолта двигает пиксели **каждой** страницы,
требует отдельного полного прогона и переустановки базовых строк
KNOWN_DEBTORS почти по всему набору, а также проверки, что типографика
chrome (DS: Golos Text / JetBrains Mono приходят пустым `font_family`) не
поедет. Это отдельная задача поверх уже влитого механизма.

Смежный остаток, вскрытый попутно: **конкретные системные семейства**
(`font-family: "Times New Roman"`, `Arial`) рендер выбирает правильно, а
`MultiFontMeasurer` их не знает — меряет bundled Inter-ом. Работает
косвенно (`Arial, sans-serif` меряется Arial-ом через generic-хвост), но
`font-family: Arial` без generic-хвоста — нет.
