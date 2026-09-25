# BUG-625

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** shell (`main.rs::relayout_chrome_host`), chrome
(`assets/chrome/chrome.html` — `--font-ui` / `--font-mono`)
**Файл:** `crates/shell/src/main.rs` (`relayout_chrome_host`, строка с
`lumen_paint::FontMeasurer::new(&font)`)

## Описание

Движковый хром меряется **не тем шрифтом, которым рисуется**.

`relayout_chrome_host` строит измеритель как
`lumen_paint::FontMeasurer::new(&font)`, где `font` — bundled Inter.
`FontMeasurer` семейство-слепой: `TextMeasurer::char_width_with_families`
у него не переопределён, дефолт трейта (`layout/src/lib.rs:195`) просто
выбрасывает список семейств и меряет своим единственным face-ом. То есть
любая надпись хрома получает ширины Inter-а независимо от объявленного
`font-family`.

Рисует же хром по объявленному стеку (`assets/chrome/chrome.html`):

- `--font-ui: 'Inter', -apple-system, 'Segoe UI', Roboto, sans-serif` —
  `Renderer::resolve_face_id` не знает зарезервированного имени «Inter»,
  идёт в `FontProvider`; на машине без установленного Inter первым
  найдётся **Segoe UI**;
- `--font-mono: 'JetBrains Mono', ui-monospace, 'SFMono-Regular', Menlo,
  monospace` — «JetBrains Mono» зарезервировано (DS-4), рендер
  коротит его на **bundled JetBrains Mono** ещё до провайдера
  (`renderer.rs::resolve_face_id`, ветка reserved-имён).

Итог: моноширинные надписи хрома всегда набираются JetBrains Mono, а
размечаются метриками пропорционального Inter; UI-надписи размечаются
Inter-ом, а рисуются тем, что провайдер нашёл по стеку. Это тот же класс
рассинхрона «измеритель ≠ рендер», что и [BUG-128](BUG-128-OPEN.md), но
на другом пути: страница ходит через `MultiFontMeasurer` (там резолв
починен целиком — generic-и в ч.1, конкретные семейства в ч.3), а хром —
через голый `FontMeasurer`.

## Как найдено

2026-08-05, попутно при BUG-128 ч.3 (P3): проверялось, не сдвинет ли
ленивый резолв конкретных семейств хром. Не сдвинет — `MultiFontMeasurer`
в хром-пути вообще не участвует; заодно вскрылось, что там нет НИКАКОГО
резолва семейств.

Установлено чтением кода (путь однозначный: дефолт трейта отбрасывает
список семейств), живым кадром **не** мерялось — насколько именно
разъезжаются надписи и какие из CC-дефектов это объясняет, ещё предстоит
измерить.

## Что делать

Либо давать хром-пути тот же `MultiFontMeasurer` + `SystemFaceSet`, что и
странице (тогда нужно ещё научить набор зарезервированным bundled-именам
`Golos Text` / `Golos Text Medium` / `JetBrains Mono` — иначе останется
рассинхрон уже на них), либо зарегистрировать bundled chrome-шрифты как
семьи измерителя. Первое честнее: правило «измеритель обязан резолвить
семейство ровно так же, как рендер» тогда становится единым для обоих
документов.

Домен — хром (CC-дорожка P1), не P3-точечный фикс.

## Фикс (P3, 2026-09-25)

Выбран первый вариант из «Что делать» — хром-путь получил тот же
`MultiFontMeasurer` + `SystemFaceSet`, что и страница, плюс зарезервированные
bundled-имена:

- `MultiFontMeasurer::register_chrome_bundled_families`
  (`crates/engine/paint/src/lib.rs`) регистрирует `Golos Text` /
  `Golos Text Medium` / `JetBrains Mono` из `chrome_fonts` как
  @font-face-слоты. Слоты проверяются раньше системного имени, поэтому
  установленный в ОС одноимённый шрифт bundled не перебьёт — ровно как
  `Renderer::resolve_face_id` коротит эти имена до провайдера.
- `relayout::chrome_measurer()` (`crates/shell/src/relayout.rs`) строит такой
  измеритель один раз на процесс (веб-шрифтов у хрома нет, а перекладывается он
  на каждый hover); `relayout_chrome_host` берёт его вместо голого
  `FontMeasurer`. Заодно с функции снят унаследованный
  `#[allow(clippy::expect_used)]` — `expect` в ней больше нет.
- `chrome_fonts` вынесен из-под feature-гейта бэкендов: измеритель
  компилируется в любой конфигурации `lumen-paint`.

Остальные вызовы `FontMeasurer::new` в shell (`find_bar`, `spell_menu`,
выделение/каретка `<textarea>`, `paint_partial_dom`) меряют **страничный**
текст и к хрому не относятся; их рассинхрон со шрифтом страницы — отдельный
вопрос, в этот баг не входит.

## Замер

Раскладка `chrome.html` на 1024×720, старый измеритель против нового:
из 86 текстовых фрагментов ширина сменилась у 79 (Segoe UI на этой машине
примерно на 7 % уже Inter-а; `<kbd>Ctrl</kbd>` 16.76 → 24.00 px — ровно 4
моно-ячейки JetBrains Mono по 10px). Ни один блочный бокс не сдвинулся
(`#sidebar`, `#contentArea`, `#demoBar` — те же rect), меняются только
инлайн-позиции внутри `#demoBar` и сайдбара.

Живой кадр (`LUMEN_TEXT_SIG=2`, dev-release): `Ctrl`/`K` рисуются face-ом
id=3 (upem 1000, 270224 байт — bundled JetBrains Mono), UI-надписи —
`segoeui.ttf`/`seguisb.ttf`, то есть рендер действительно шёл по стеку, а
измеритель теперь совпадает с ним.

## Регрессия

- `multi_font_tests::chrome_bundled_families_measured_with_bundled_faces`
  (lumen-paint).
- `tests::chrome_incremental::bug625_chrome_mono_text_measured_with_bundled_jetbrains_mono`
  (lumen-shell) — на настоящем `chrome.html`; без регистрации bundled-семей
  падает с `ширина «Ctrl» 21.99 ≠ 24`.

## Гейты

`dump_golden.py` — 12/12 совпадают (хром в дампы не попадает). CPU-снапшоты
хром не рисуют. Полный `graphic_tests/run.py` в этой сессии не запускался:
TEST-00 не находит магента-маркер, и так же падает бинарь **без** фикса
(окно из процесса вне foreground-цепочки — класс BUG-1062). На пиксели
страниц правка не влияет; в живом окне сдвигается текст плавающей
`#demoBar`, которую Edge не рисует вообще (BUG-1077), — ратчеты TEST-57/157
могут шевельнуться в пределах уже приписанного ей дифа.
