# BUGS — реестр визуальных багов Lumen

Журнал багов, найденных при сравнении Lumen vs Edge headless на `graphic_tests/`.

Формат: `BUG-NNN` сквозная нумерация. Status: `OPEN` / `FIXED <дата>` / `WONTFIX <причина>`.

Скрины (`graphic_tests/screenshots/*.png`) **не коммитятся** (см. `.gitignore`). Здесь — только описания.

Текст игнорируем (антиалиасинг расходится с Edge by design). Серые scrollbar-полосы по правому/нижнему краю Edge-скринов — артефакт Edge headless, **не баг Lumen**.

---

## Прогон 2026-05-19

Прогнаны страницы 00 (calibration) + 01–20 + `1000000-final.html` через `python graphic_tests/run.py`. Пайплайн блокирующий — остановился на тесте 02 (color-named) с diff 29.30% (порог 5%).

Найдены 11 реальных багов (системные / критичные) + 1 артефакт workflow (BUG-012, WONTFIX → устранён через 00-calibration).

**Тесты 00 и 01 проходят pixel-perfect (0.00% diff).** Это значит:
- Магента-маркер ✓ рендерится корректно (block + class CSS background-color + width/height работают).
- Сдвиг (-8, -16), наблюдавшийся ранее, был артефактом жёсткого crop offset, не Lumen.
- Базовая геометрия позиционирования через `margin-top` / `margin-left` ✓ работает.

Дальше начинаются проблемы (см. BUG-001 и BUG-002).

### BUG-001 — `display: inline-block` не выкладывается в строку

**Status:** OPEN
**Severity:** CRITICAL (блокирует половину тестов)
**Pages:** 02, 03, 04, 05, 06, 12, 13, 15, 16, 18, 19, 20, final

Edge: `display: inline-block` элементы располагаются по горизонтали в одну строку, переносятся на следующую при нехватке ширины.

Lumen: элементы либо стакаются вертикально как `display: block`, либо схлопываются в 0×0 (как `display: inline` на пустом div). Симптомы:

- **02 / 03 / 04 (colors)** — все 18 / 9 / 18 цветных квадратиков пропадают: одна paint-команда (только body), белая страница.
- **05 (border-width)** — 10 inline-block div-ов вместо двух рядов рисуются единственным тонким столбцом слева на всю высоту viewport. 21 paint-команда — то есть рендер происходит, но позиционирование/размер сломаны.
- **12 (display)** — полностью пустая страница (только body): даже `display: block; width: ...; background: ...;` в inline-style не работает, см. BUG-002.
- **15 (box-shadow)** — 8 inline-block боксов в виде вертикальной колонки из ~135×80 серых прямоугольников вместо горизонтального ряда.
- **16 (outline)** — то же, что 05: один тёмный столбец на всю высоту слева вместо двух рядов.
- **18 / 19 (images)** — картинки стакаются в левую колонку (4 видны), правые 6–8 за viewport-ом.
- **final (1000000)** — большая часть содержимого теряется: видны только 3 маленьких прямоугольника в левой колонке.

**Гипотеза:** `display: inline-block` парсится как `display: inline` (fallback на дефолт div-а как inline?) → пустой div получает 0×0, ширина/высота не применяются. Либо парсится как `block`, но без horizontal flow и `width: auto = 100%`.

---

### BUG-002 — Inline `style="..."` атрибут не применяет ключевые свойства

**Status:** OPEN
**Severity:** CRITICAL
**Pages:** 02, 03, 04, 07, 09, 10, 11, 12, 13, 18, 19, 20, final

Inline-атрибут `style="..."` парсится частично: `border-width` оттуда применяется (тест 05 — разные толщины видны), но `display`, `width`, `height`, `background` / `background-color`, `margin-*`, `padding`, `min-width`/`max-width` — **не применяются**.

- **12 (display)** — все стили в inline (`style="display: block; width: 480px; height: 40px; background: #3182ce"`). Lumen рендерит пустую страницу. Если бы inline-style работал, мы бы как минимум увидели 3 синие полосы display:block.
- **10 (min-max-width)** — `style="width: ...; min-width: ...; max-width: ..."` не применяется → все 12 баров рендерятся на полную ширину (1024px). В Edge видна сетка с разными ширинами.
- **09 (margin)** — `style="margin-left: 25px;"` не применяется → диагональной «лесенки» из 5 синих баров нет, все на одной позиции x=0.
- **02 / 03 / 04 (colors)** — `style="background: red;"` не применяется → ни один из цветных квадратиков не виден.

**Гипотеза:** парсер inline-style игнорирует свойства, не входящие в узкий whitelist (border-* видимо в списке). Либо `style="..."` парсится только в составе `<input>` / некоторого типа элементов.

---

### BUG-003 — `background` shorthand или color-named values в inline-style не парсятся

**Status:** OPEN
**Severity:** HIGH (часть BUG-002, но отдельный аспект)
**Pages:** 02, 03, 04

Даже если бы BUG-002 был починен в части background, явно видно, что `background: red`, `background: rgb(255,0,0)`, `background: rgba(255,0,0,0.5)` из inline-style не применяются. На страницах 02–04 ВСЕ ЦВЕТА в inline. Результат — белая страница (только body background работает из `<style>`-блока).

Возможно, проблема в shorthand `background` (vs `background-color`). Также возможно, named colors (`red`, `blue`, `tomato`) парсятся хуже, чем `#hex` (на 04 половина цветов — rgb/rgba, на 03 — все hex).

---

### BUG-004 — `margin-left` не сдвигает блочные элементы

**Status:** OPEN
**Severity:** HIGH
**Pages:** 09

В Edge на странице 09 видна «диагональная лесенка» из 5 синих баров: каждый сдвинут вправо на 25px относительно предыдущего за счёт `margin-left: 0/25/50/75/100px`. В Lumen — 5 одинаковых полос шириной ~415px, все начинаются от x=0.

Также не виден ряд красно-зелёных полос (margin-collapse test) — возможно, тут другой косяк layout, нужно дальше копать.

(Связано с BUG-002, но margin-left на block-element заслуживает отдельной верификации, потому что должен работать даже если inline-block сломан.)

---

### BUG-005 — `width` / `height` из inline-style не уменьшают блочный элемент

**Status:** OPEN
**Severity:** CRITICAL
**Pages:** 07, 09, 10, 11, 12, 13

Когда block-элемент имеет inline `style="width: 200px"`, в Lumen он рендерится на полную ширину (1024px), а не на 200px. В Edge ограничение работает.

- **07 (box-sizing)** — 8 пар red/blue боксов с `width: 200px` или `width: 320px` в inline → в Lumen все рендерятся как полнополосные бары по 1024px. Различие content-box vs border-box не видно вообще, так как боксов нужного размера нет.
- **10, 11 (min-max-width/height)** — то же: размеры не применяются.

---

### BUG-006 — `box-shadow` не рендерится

**Status:** OPEN
**Severity:** HIGH (новая фича)
**Pages:** 15

В Edge на странице 15 видны 8 боксов с разными тенями: смещение (gray/red/black), blur, multi-shadow, цветная shadow, inset shadow. В Lumen — голые прямоугольники без теней (плюс BUG-001: они стакаются вертикально). Свойство `box-shadow` похоже не реализовано в paint pipeline.

---

### BUG-007 — `outline` не отрисовывается на боксе

**Status:** OPEN
**Severity:** HIGH (новая фича / частично)
**Pages:** 16

В Edge на странице 16 видны 11 боксов с цветными `outline` (белая 1/2/4 px, светло-серая, красная, зелёная, жёлтая, голубая, розовая, оранжевая, double, dashed). В Lumen — белые/светлые прямоугольники без видимой контурной обводки.

Замечу: outline-задачи частично сделаны (см. commit `5a527a3` reservation outline-dash-dot и `5a527a3`/`2e683a2` про dashed/dotted), но визуально не виден ни один outline-style. Возможно реализован только display-list, без paint, или paint не подключён.

---

### BUG-008 — `overflow: scroll/auto/hidden` не реализован

**Status:** OPEN
**Severity:** MEDIUM (Phase 0 не обязан)
**Pages:** 14

В Edge на странице 14 видны вложенные элементы с overflow: scrollbar-полоски внутри отдельных боксов, обрезка контента по бордюру. В Lumen — простой бокс без внутренних скроллбаров и без клиппинга. Свойство `overflow` пока обрабатывается как `visible`.

---

### BUG-009 — HTML-атрибут `bgcolor` не применяется

**Status:** OPEN
**Severity:** HIGH (нужен для quirks legacy)
**Pages:** 20

Body имеет `<body bgcolor="#1a2030">` (HTML presentational hint). В Edge body — тёмно-синий. В Lumen — белый. Атрибут `bgcolor` не транслируется в стиль. То же для `<td bgcolor="...">` — все ячейки таблицы не закрашены.

---

### BUG-010 — Hashless hex colors (quirks-mode) не парсятся

**Status:** OPEN
**Severity:** LOW (quirks legacy)
**Pages:** 20

`background: ff4444` (без `#`) в quirks-mode должен парситься как `#ff4444`. В Lumen — игнорируется. Edge применяет.

---

### BUG-011 — `<table>` / `<tr>` / `<td>` не рендерятся

**Status:** OPEN
**Severity:** HIGH
**Pages:** 20, потенциально другие

Страница 20 содержит `<table>` с 10 ячейками. В Edge видна сетка 5×2. В Lumen — пустая страница (вместе с body bgcolor). Tag-ы таблицы либо не учитываются в layout (нет table flow), либо вообще не доходят до paint.

---

### BUG-012 — Сдвиг квадрата (00-sanity)

**Status:** FIXED 2026-05-19 (артефакт workflow, устранён через 00-calibration.html)

Изначально показалось сдвигом квадрата на (-8, -16) px. Корень — жёсткий crop offset `crop=...:8:39`, не отражающий реальную позицию окна на десктопе.

**Устранено:** новый тест `00-calibration.html` рендерит магента-маркеры по верху и низу viewport, workflow `graphic_tests/run.py` находит их в desktop-снимке Lumen и определяет точные координаты content area динамически. После этого тест 01-sanity (бывший 00) показывает 0.00% diff с Edge — pixel-perfect.

---

## Сводка по страницам (прогон 2026-05-19)

| # | Файл | diff% | Статус | Связанные баги |
|---|---|---:|---|---|
| 00 | 00-calibration.html | 0.00 | ✅ PASS | — |
| 01 | 01-sanity.html | 0.00 | ✅ PASS | — |
| 02 | 02-color-named.html | 29.30 | ❌ FAIL → STOP | BUG-001, BUG-002, BUG-003 |
| 03 | 03-color-formats.html | — | ⏸ пропущен (пайплайн остановлен) | BUG-001, BUG-002, BUG-003 |
| 04 | 04-color-alpha.html | — | ⏸ пропущен | BUG-001, BUG-002, BUG-003 |
| 05 | 05-border-width.html | — | ⏸ пропущен | BUG-001 |
| 06 | 06-border-sides.html | — | ⏸ пропущен | BUG-001 |
| 07 | 07-box-sizing.html | — | ⏸ пропущен | BUG-002, BUG-005 |
| 08 | 08-padding.html | — | ⏸ пропущен | BUG-001, BUG-002 |
| 09 | 09-margin.html | — | ⏸ пропущен | BUG-002, BUG-004 |
| 10 | 10-min-max-width.html | — | ⏸ пропущен | BUG-002, BUG-005 |
| 11 | 11-min-max-height.html | — | ⏸ пропущен | BUG-002, BUG-005 |
| 12 | 12-display.html | — | ⏸ пропущен | BUG-002 |
| 13 | 13-visibility-opacity.html | — | ⏸ пропущен | BUG-001, BUG-002 |
| 14 | 14-overflow.html | — | ⏸ пропущен | BUG-008 |
| 15 | 15-box-shadow.html | — | ⏸ пропущен | BUG-001, BUG-006 |
| 16 | 16-outline.html | — | ⏸ пропущен | BUG-001, BUG-007 |
| 17 | 17-calc.html | — | ⏸ пропущен | BUG-002, BUG-005 |
| 18 | 18-images.html | — | ⏸ пропущен | BUG-001 |
| 19 | 19-object-fit.html | — | ⏸ пропущен | BUG-001 |
| 20 | 20-quirks-bgcolor.html | — | ⏸ пропущен | BUG-008, BUG-009, BUG-010, BUG-011 |
| ∞ | 1000000-final.html | — | (ручная проверка) | BUG-001 + др. |

**Общий вывод:** базовая геометрия (тесты 00/01) — pixel-perfect. Ядро layout/inline-style сломано: BUG-001 (`display: inline-block`) и BUG-002 (inline-`style="..."` атрибут) — root cause для ~80% наблюдаемых проблем. После их починки стоит перезапустить пайплайн — большая часть пропущенных тестов должна либо пройти, либо проявить отдельные более узкие баги.
