# BUG-333: flex-строки хрома схлопывались в `h=0` (заголовок «виноват `var()`» — опровергнут)

**Статус:** FIXED 2026-07-29 (P1)
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs::lay_out_flex`)
**Найден:** P1, CC-1 (`docs/tasks/p1-css-chrome.md`), скрин-сверка `about:chrome-preview` с эталоном `docs/design/lumen-v3_3.html` 2026-07-24

> **Внимание при чтении:** исходный заголовок и раздел «Симптом» ниже приписывают
> дефект резолюции `var()` в `height`. Это **неверно** — см. «Диагноз» в конце файла.
> Причина оказалась общей flex-багой, тождественной [BUG-343](BUG-343-FIXED.md).

## Симптом

В сайдбаре хрома (`.sb-tabs` — список вкладок под `.sb-workspaces`) все строки `.tab-row`
рендерятся с нулевой высотой: текст заголовков соседних вкладок налезает друг на друга
(скриншот `about:chrome-preview`, область сайдбара y≈180–220 при viewport 1024×720).

`--dump-layout about:chrome-preview` подтверждает — каждая `.tab-row` имеет `h=0.00`:

```
Block rect=(8.00, 191.92, 223.00, 0.00) bg=#f0f2f5ff position=relative display=flex h=0.00 ...
```

при CSS-правиле:

```css
:root{ --tab-h:28px; --toolbar-h:36px; }
.tab-row{
  display:flex; align-items:center; gap:6px; height:var(--tab-h); padding:0 6px 0 8px;
  border-radius:var(--radius-sm); position:relative; border-left:2px solid transparent; cursor:pointer;
}
```

Контрольный пример на той же странице — `.toolbar{ height:var(--toolbar-h); ... }` — резолвится
**корректно** (`h=36.00` в дампе), т.е. `var()` в `height` в принципе работает; ломается только
в контексте `.tab-row`.

## Воспроизведение

1. `cargo run -p lumen-shell -- --dump-layout about:chrome-preview` (ветка `p1-cc-1-chrome-assets-smoke`,
   нужен `assets/chrome/chrome.html`, генерируется `scripts/gen_chrome_assets.py`).
2. В выводе найти блоки `bg=#f0f2f5ff position=relative display=flex h=0.00 ... bs=(none,none,none,solid)`
   в секции сайдбара (после `ВКЛАДКИ · ЛИЧНОЕ`) — это `.tab-row`, все с `h=0.00`.

Изолированный минимальный репро **не собрался** за разумное время диагностики — три попытки:

- Тот же CSS для `.tab-row`/`.sb-tabs`/`.sidebar` (без остальных ~530 строк файла) на 3 строках —
  **работает корректно** (`h=28.00`).
- Реальная разметка (`sed -n '628,693p' assets/chrome/chrome.html`, все 10 `.tab-row` с
  `data-ws`/`.active`/`.child`/`.sleeping`/`.tree-line`) + написанный вручную аналогичный CSS —
  **тоже работает корректно**.
- CSS-строки 13–140 исходного `assets/chrome/chrome.html` (`:root` + база `body` + `.sidebar`/
  `.sb-profile`/`.sb-workspaces`/`.tab-row` включительно) + **дословная** разметка `<aside
  class="sidebar">` (строки 589–702) — **воспроизводится** (`h=0.00`).
- Попытка добавить к упрощённому репро «дробный остаток» соседей `flex:1` (имитация того, что
  `.sb-tabs` в реальной странице получает `flex:1` от родителя высотой `511.08px`, не целым
  числом, из-за предшествующих `.sb-profile`/`.sb-divider`/`.sb-workspaces`) — **не** воспроизвела
  баг в одиночку.

Т.е. триггер требует полного каскада (не одного правила `.tab-row` — что-то в CSS-строках 13–140
в сочетании с точной вложенностью реальной разметки `.sidebar > .sb-profile + .sb-divider +
.sb-workspaces + .sb-tabs`). Самый надёжный воспроизводимый кейс на сегодня — дословное извлечение
`sed -n '1,12p;13,140p;589,702p' assets/chrome/chrome.html` в отдельный `<html>` (обёрнутый
`</style></head><body>...</body></html>`).

## Диагноз (P1, 2026-07-29)

### Заявка про `var()` опровергнута

Первый же дискриминирующий опыт: в воспроизводимом извлечении заменить
`height:var(--tab-h)` на дословный `height:28px`. Результат — **та же `h=0.00`**.
`var()` в цепочке не участвует; `.toolbar` работал не потому, что «там var()
резолвится», а потому что у него `flex:none` в другом контейнере.

Второе, что вводило в заблуждение: `h=` в `--dump-layout` печатается из
`style.height` (`snapshot.rs`), а flex-раскладка **записывает** туда резолвленный
px. Поэтому `h=0.00` — это не «в стиле стоит 0», а «flex вычислил 0 и записал».
Признак записи виден прямо в дампе: `.sb-section-label` без единой декларации
`height` печатался как `h=12.00`.

### Корень

Резолвленный used-размер flex-item'а передаётся в рекурсивный `lay_out` не
аргументом, а записью в стиль самого item'а (`box_sizing:border-box` + явный
`Length::Px`) — аргумент `available_height` у `lay_out` означает размер
containing block для процентов, а не переопределение размера бокса. Запись
не откатывалась, а одно и то же поддерево движок раскладывает несколько раз
с разным доступным местом:

1. `.app` (row flex) видит `.sidebar` как item с `flex:none` (→ `FlexBasis::Auto`)
   и явной `width` → `needs_prelayout == true` → Step-1 проба
   `lay_out(.sidebar, …, available_height = None)`.
2. Внутри пробы `.sidebar` (column flex) не имеет своей `height` → `explicit_main
   = None` → `main_definite = None` → `container_main = 0`.
3. `.sb-tabs` (`flex:1` → `FlexBasis::Length(0%)`; `overflow-y:auto`, поэтому
   automatic minimum size §4.5 отключён) → `all_hyp = 0` → `inner_main = 0` →
   `.sb-tabs.style.height = 0px`, и она раскладывается с **определённым** main = 0.
4. Её `.tab-row` (`height:28px`) получают `free_space = 0 − 280 − gaps < 0` →
   shrink с клампом `.max(0.0)` → 0 → `.tab-row.style.height = 0px` **навсегда**.
5. Настоящий проход `.app`, где `.sb-tabs` получает 511.08px, уже ничего не
   пересчитает: в стиле строк стоит жёсткий `0px`, исходные `28px` уничтожены.

Это в точности механизм [BUG-343](BUG-343-FIXED.md) — оба закрыты одним фиксом.

## Фикс

`SavedItemSizing` (`box_tree.rs`, рядом с `lay_out_flex`) снимает
`width`/`height`/`box_sizing` до записи used-размера и возвращает после
`lay_out` — все три места: main-ось column, main-ось row, cross-растяжение
(`relayout_column_flex`). Спецификация переживает пробный проход, каждый
проход резолвит проценты и явные размеры заново.

Побочный эффект: `--dump-layout` теперь печатает `w=`/`h=` **как объявлено**
(`h=28.00`, `w=100.00%`), а не как резолвлено — резолвленная геометрия и раньше
читалась из `rect=(…)`.

## Проверено

- `--dump-layout about:chrome-preview`: все `.tab-row` → `h=28.00`, `rect` высотой
  28 (было `h=0.00`); `.toolbar` без изменений (36px).
- Регресс-тесты `flex_probe_pass_does_not_burn_item_height_into_style` и
  `flex_probe_pass_does_not_burn_percentage_width_into_style` — оба падают без
  фикса ровно исходными симптомами (`got 0` и `got 300`).
