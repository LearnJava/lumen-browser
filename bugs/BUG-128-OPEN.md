# BUG-128

**Статус:** OPEN (DEBTOR) — механизм резолва, дефолт документа и измеритель
конкретных системных семейств починены; осталась только нередуцируемая
глиф-парити с Edge (FP-1)
**Компонент:** font
**Файл:** — (точечного дефекта не осталось; остаток держится на FP-1)

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

## Что починено 2026-08-05, ч.3 (P3, ветка `p3-bug-128-named-families`)

Измеритель конкретных системных семейств. Было: `MultiFontMeasurer` знал
только @font-face-семьи и шесть generic-ов (ч.1), а рендер резолвил через
`FontProvider::pick_face` **любое** имя — поэтому `font-family: Arial` без
generic-хвоста мерился bundled Inter-ом, а рисовался Arial-ом.

Замерено на живой сборке (`--dump-layout`, shrink-to-fit `inline-block` с
одной и той же строкой):

| объявление | до | после |
|---|---|---|
| `font-family: Arial` | 153.37px (Inter) | 144.00px |
| `font-family: Arial, sans-serif` | 144.00px | 144.00px |
| `font-family: 'Times New Roman'` | 153.37px (Inter) | 143.52px |
| `font-family: 'Times New Roman', serif` | 143.52px | 143.52px |
| `font-family: Consolas` | 153.37px (Inter) | 167.14px |
| `font-family: Consolas, monospace` | 167.14px | 167.14px |

То есть до фикса ширина зависела от того, дописан ли generic-хвост, хотя
рисовался в обоих случаях один и тот же face.

Стало:
- `GenericFaceSet` → `SystemFaceSet` (`lumen_paint`): шесть generic-ов
  по-прежнему резолвятся при постройке, конкретные имена — **лениво** при
  первом измерении через `FontProvider::pick_face`. Вперёд их читать
  нельзя: список установленных в системе семейств открыт;
- набор процесс-глобален (`OnceLock` в шелле) и передаётся измерителю
  `Arc`-ом, поэтому ленивый кэш переживает пересборку измерителя — файл
  шрифта читается и парсится один раз на процесс, а не на relayout;
- промахи кэшируются так же, как попадания: иначе `font-family: Helvetica`
  на Windows опрашивал бы системный индекс на каждый измеряемый символ;
- предел `MAX_CACHED_NAMED_FACES = 64` на РЕЗОЛВЛЕННЫЕ метрики: запись —
  это cmap + таблица advance-ов (сотни килобайт), а страница может
  перечислить тысячу выдуманных имён. Сверх предела имя меряется bundled
  Inter-ом; отрицательные записи предел не расходуют (стоят одну строку);
- нерезолвленный generic в системный индекс не отдаётся (`is_generic_family`
  отсекает): шрифта с family name «serif» не ставит ни одна ОС.

Chrome-документ не задет: `relayout_chrome_host` меряет обычным
`FontMeasurer` (bundled Inter), `MultiFontMeasurer` там не участвует.
Графические тесты не сдвинулись — ни один из них не объявляет конкретное
семейство без generic-хвоста (единственное вхождение —
`font-family: Arial, sans-serif`, которое и до фикса мерилось Arial-ом
через хвост).

## Что осталось (это и есть сам баг)

**Нередуцируемая глиф-парити с Edge.** Оставшиеся проценты должников
класса font-parity (TEST-79 5.64% и др.) — это разница растеризации/
хинтинга одного и того же Times New Roman, а не выбор шрифта. Закроется
только вместе с FP-1 (единая политика рендера глифов, домен P1), не
точечным фиксом: в самом баге P3-работы больше не осталось, строка
`BUGS.md:146` держится в `STATUS-P3.md` только как якорь записей
`KNOWN_DEBTORS`.

Записи `KNOWN_DEBTORS` с тегом BUG-128 (32/34/46/52/64/66/67/79/80/82/84/
95/97) держатся на этой глиф-парити — поэтому баг остаётся OPEN.
